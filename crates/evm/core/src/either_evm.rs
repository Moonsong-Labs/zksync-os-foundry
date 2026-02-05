use alloy_evm::{
    Database, EthEvm, Evm, EvmEnv, FromRecoveredTx, IntoTxEnv, InvalidTxError, eth::EthEvmContext,
};
use alloy_op_evm::OpEvm;
use alloy_primitives::{Address, Bytes};
use alloy_zksync_os_evm::ZKsyncEvm;
use foundry_primitives::FoundryTxEnvelope;
use op_revm::{OpContext, OpHaltReason, OpSpecId, OpTransaction, OpTransactionError};
use revm::{
    DatabaseCommit, Inspector,
    context::{
        BlockEnv, TxEnv,
        result::{
            EVMError, ExecResultAndState, ExecutionResult, HaltReason, InvalidTransaction,
            ResultAndState,
        },
    },
    handler::PrecompileProvider,
    interpreter::InterpreterResult,
    primitives::hardfork::SpecId,
};
use zksync_os_revm::{ZKsyncTx, ZKsyncTxError, ZkContext, ZkSpecId};

/// Alias for result type returned by [`Evm::transact`] methods.
type EitherEvmResult<DBError, HaltReason, TxError> =
    Result<ResultAndState<HaltReason>, EVMError<DBError, TxError>>;

/// Alias for result type returned by [`Evm::transact_commit`] methods.
type EitherExecResult<DBError, HaltReason, TxError> =
    Result<ExecutionResult<HaltReason>, EVMError<DBError, TxError>>;

/// [`EitherEvm`] delegates its calls to one of the two evm implementations; either [`EthEvm`] or
/// [`OpEvm`].
///
/// Calls are delegated to [`OpEvm`] only if optimism is enabled.
///
/// The call delegation is handled via its own implementation of the [`Evm`] trait.
///
/// The [`Evm::transact`] and other such calls work over the [`OpTransaction<TxEnv>`] type.
///
/// However, the [`Evm::HaltReason`] and [`Evm::Error`] leverage the optimism [`OpHaltReason`] and
/// [`OpTransactionError`] as these are supersets of the eth types. This makes it easier to map eth
/// types to op types and also prevents ignoring of any error that maybe thrown by [`OpEvm`].
#[allow(clippy::large_enum_variant)]
pub enum EitherEvm<DB, I, P>
where
    DB: Database,
{
    /// [`EthEvm`] implementation.
    Eth(EthEvm<DB, I, P>),
    /// [`OpEvm`] implementation.
    Op(OpEvm<DB, I, P>),
    /// [`ZKsyncEvm`] implementation.
    ZKsync(ZKsyncEvm<DB, I, P>),
}

impl<DB, I, P> EitherEvm<DB, I, P>
where
    DB: Database,
    I: Inspector<EthEvmContext<DB>> + Inspector<OpContext<DB>>,
    P: PrecompileProvider<EthEvmContext<DB>, Output = InterpreterResult>
        + PrecompileProvider<OpContext<DB>, Output = InterpreterResult>,
{
    /// Converts the [`EthEvm::transact`] result to [`EitherEvmResult`].
    fn map_either_evm_eth_result(
        &self,
        result: Result<ExecResultAndState<ExecutionResult>, EVMError<DB::Error>>,
    ) -> EitherEvmResult<DB::Error, EitherHaltReason, EitherTxError> {
        match result {
            Ok(result) => Ok(ResultAndState::<EitherHaltReason, _> {
                result: result.result.map_haltreason(EitherHaltReason::Eth),
                state: result.state,
            }),
            Err(e) => Err(match e {
                EVMError::Transaction(invalid_tx) => {
                    EVMError::Transaction(EitherTxError::Eth(invalid_tx))
                }
                EVMError::Database(e) => EVMError::Database(e),
                EVMError::Header(e) => EVMError::Header(e),
                EVMError::Custom(e) => EVMError::Custom(e),
            }),
        }
    }

    /// Converts the [`EthEvm::transact`] result to [`EitherEvmResult`].
    fn map_either_evm_op_result(
        &self,
        result: Result<
            ExecResultAndState<ExecutionResult<OpHaltReason>>,
            EVMError<DB::Error, OpTransactionError>,
        >,
    ) -> EitherEvmResult<DB::Error, EitherHaltReason, EitherTxError> {
        match result {
            Ok(result) => Ok(ResultAndState::<EitherHaltReason, _> {
                result: result.result.map_haltreason(EitherHaltReason::Op),
                state: result.state,
            }),
            Err(e) => Err(match e {
                EVMError::Transaction(invalid_tx) => {
                    EVMError::Transaction(EitherTxError::Op(invalid_tx))
                }
                EVMError::Database(e) => EVMError::Database(e),
                EVMError::Header(e) => EVMError::Header(e),
                EVMError::Custom(e) => EVMError::Custom(e),
            }),
        }
    }

    fn map_either_evm_zksync_result(
        &self,
        result: Result<
            ExecResultAndState<ExecutionResult<HaltReason>>,
            EVMError<DB::Error, ZKsyncTxError>,
        >,
    ) -> EitherEvmResult<DB::Error, EitherHaltReason, EitherTxError> {
        match result {
            Ok(result) => Ok(ResultAndState::<EitherHaltReason, _> {
                result: result.result.map_haltreason(EitherHaltReason::ZKsync),
                state: result.state,
            }),
            Err(e) => Err(match e {
                EVMError::Transaction(invalid_tx) => {
                    EVMError::Transaction(EitherTxError::ZKsync(invalid_tx))
                }
                EVMError::Database(e) => EVMError::Database(e),
                EVMError::Header(e) => EVMError::Header(e),
                EVMError::Custom(e) => EVMError::Custom(e),
            }),
        }
    }

    /// Converts the [`EthEvm::transact_commit`] result to [`EitherExecResult`].
    fn map_either_evm_exec_eth_result(
        &self,
        result: Result<ExecutionResult, EVMError<DB::Error>>,
    ) -> EitherExecResult<DB::Error, EitherHaltReason, EitherTxError> {
        match result {
            Ok(result) => Ok(result.map_haltreason(EitherHaltReason::Eth)),
            Err(e) => Err(match e {
                EVMError::Transaction(invalid_tx) => {
                    EVMError::Transaction(EitherTxError::Eth(invalid_tx))
                }
                EVMError::Database(e) => EVMError::Database(e),
                EVMError::Header(e) => EVMError::Header(e),
                EVMError::Custom(e) => EVMError::Custom(e),
            }),
        }
    }

    fn map_either_evm_exec_op_result(
        &self,
        result: Result<ExecutionResult<OpHaltReason>, EVMError<DB::Error, OpTransactionError>>,
    ) -> EitherExecResult<DB::Error, EitherHaltReason, EitherTxError> {
        match result {
            Ok(result) => Ok(result.map_haltreason(EitherHaltReason::Op)),
            Err(e) => Err(match e {
                EVMError::Transaction(invalid_tx) => {
                    EVMError::Transaction(EitherTxError::Op(invalid_tx))
                }
                EVMError::Database(e) => EVMError::Database(e),
                EVMError::Header(e) => EVMError::Header(e),
                EVMError::Custom(e) => EVMError::Custom(e),
            }),
        }
    }

    fn map_either_evm_exec_zksync_result(
        &self,
        result: Result<ExecutionResult<HaltReason>, EVMError<DB::Error, ZKsyncTxError>>,
    ) -> EitherExecResult<DB::Error, EitherHaltReason, EitherTxError> {
        match result {
            Ok(result) => Ok(result.map_haltreason(EitherHaltReason::ZKsync)),
            Err(e) => Err(match e {
                EVMError::Transaction(invalid_tx) => {
                    EVMError::Transaction(EitherTxError::ZKsync(invalid_tx))
                }
                EVMError::Database(e) => EVMError::Database(e),
                EVMError::Header(e) => EVMError::Header(e),
                EVMError::Custom(e) => EVMError::Custom(e),
            }),
        }
    }

    /// Converts the [`EthEvm::transact`] result to [`EitherEvmResult`].
    #[allow(unused)]
    fn map_eth_result(
        &self,
        result: Result<ExecResultAndState<ExecutionResult>, EVMError<DB::Error>>,
    ) -> EitherEvmResult<DB::Error, OpHaltReason, OpTransactionError> {
        match result {
            Ok(result) => Ok(ResultAndState {
                result: result.result.map_haltreason(OpHaltReason::Base),
                state: result.state,
            }),
            Err(e) => Err(self.map_eth_err(e)),
        }
    }

    /// Converts the [`EthEvm::transact_commit`] result to [`EitherExecResult`].
    #[allow(unused)]
    fn map_exec_result(
        &self,
        result: Result<ExecutionResult, EVMError<DB::Error>>,
    ) -> EitherExecResult<DB::Error, OpHaltReason, OpTransactionError> {
        match result {
            Ok(result) => {
                // Map the halt reason
                Ok(result.map_haltreason(OpHaltReason::Base))
            }
            Err(e) => Err(self.map_eth_err(e)),
        }
    }

    /// Maps [`EVMError<DBError>`] to [`EVMError<DBError, OpTransactionError>`].
    #[allow(unused)]
    fn map_eth_err(&self, err: EVMError<DB::Error>) -> EVMError<DB::Error, OpTransactionError> {
        match err {
            EVMError::Transaction(invalid_tx) => {
                EVMError::Transaction(OpTransactionError::Base(invalid_tx))
            }
            EVMError::Database(e) => EVMError::Database(e),
            EVMError::Header(e) => EVMError::Header(e),
            EVMError::Custom(e) => EVMError::Custom(e),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EitherTx {
    Eth(TxEnv),
    Op(OpTransaction<TxEnv>),
    ZKsync(ZKsyncTx<TxEnv>),
}

impl Default for EitherTx {
    fn default() -> Self {
        Self::Op(Default::default())
    }
}

impl FromRecoveredTx<FoundryTxEnvelope> for EitherTx {
    fn from_recovered_tx(tx: &FoundryTxEnvelope, _caller: Address) -> Self {
        // NOTE(zk): implement in future
        match tx {
            _ => panic!("unsupported on zksync"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EitherTxError {
    Eth(InvalidTransaction),
    Op(OpTransactionError),
    ZKsync(ZKsyncTxError),
}

impl std::fmt::Display for EitherTxError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EitherTxError::Eth(tx_error) => tx_error.fmt(f),
            EitherTxError::Op(tx_error) => tx_error.fmt(f),
            EitherTxError::ZKsync(tx_error) => tx_error.fmt(f),
        }
    }
}

impl core::error::Error for EitherTxError {}

impl InvalidTxError for EitherTxError {
    fn as_invalid_tx_err(&self) -> Option<&InvalidTransaction> {
        match self {
            EitherTxError::Eth(tx_error) => tx_error.as_invalid_tx_err(),
            EitherTxError::Op(tx_error) => tx_error.as_invalid_tx_err(),
            EitherTxError::ZKsync(tx_error) => tx_error.as_invalid_tx_err(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EitherHaltReason {
    Eth(HaltReason),
    Op(OpHaltReason),
    ZKsync(HaltReason),
}

impl IntoTxEnv<EitherTx> for EitherTx {
    fn into_tx_env(self) -> EitherTx {
        self
    }
}

impl From<HaltReason> for EitherHaltReason {
    fn from(value: HaltReason) -> Self {
        Self::Eth(value)
    }
}

impl<DB, I, P> Evm for EitherEvm<DB, I, P>
where
    DB: Database,
    I: Inspector<EthEvmContext<DB>> + Inspector<OpContext<DB>> + Inspector<ZkContext<DB>>,
    P: PrecompileProvider<EthEvmContext<DB>, Output = InterpreterResult>
        + PrecompileProvider<OpContext<DB>, Output = InterpreterResult>
        + PrecompileProvider<ZkContext<DB>, Output = InterpreterResult>,
{
    type DB = DB;
    type Error = EVMError<DB::Error, EitherTxError>;
    type HaltReason = EitherHaltReason;
    type Tx = EitherTx;
    type Inspector = I;
    type Precompiles = P;
    type Spec = SpecId;
    type BlockEnv = BlockEnv;

    fn block(&self) -> &BlockEnv {
        match self {
            Self::Eth(evm) => evm.block(),
            Self::Op(evm) => evm.block(),
            Self::ZKsync(evm) => evm.block(),
        }
    }

    fn chain_id(&self) -> u64 {
        match self {
            Self::Eth(evm) => evm.chain_id(),
            Self::Op(evm) => evm.chain_id(),
            Self::ZKsync(evm) => evm.chain_id(),
        }
    }

    fn components(&self) -> (&Self::DB, &Self::Inspector, &Self::Precompiles) {
        match self {
            Self::Eth(evm) => evm.components(),
            Self::Op(evm) => evm.components(),
            Self::ZKsync(evm) => evm.components(),
        }
    }

    fn components_mut(&mut self) -> (&mut Self::DB, &mut Self::Inspector, &mut Self::Precompiles) {
        match self {
            Self::Eth(evm) => evm.components_mut(),
            Self::Op(evm) => evm.components_mut(),
            Self::ZKsync(evm) => evm.components_mut(),
        }
    }

    fn db_mut(&mut self) -> &mut Self::DB {
        match self {
            Self::Eth(evm) => evm.db_mut(),
            Self::Op(evm) => evm.db_mut(),
            Self::ZKsync(evm) => evm.db_mut(),
        }
    }

    fn into_db(self) -> Self::DB
    where
        Self: Sized,
    {
        match self {
            Self::Eth(evm) => evm.into_db(),
            Self::Op(evm) => evm.into_db(),
            Self::ZKsync(evm) => evm.into_db(),
        }
    }

    fn finish(self) -> (Self::DB, EvmEnv<Self::Spec>)
    where
        Self: Sized,
    {
        match self {
            Self::Eth(evm) => evm.finish(),
            Self::Op(evm) => {
                let (db, env) = evm.finish();
                (db, map_env(env))
            }
            Self::ZKsync(evm) => {
                let (db, env) = evm.finish();
                (db, map_zksync_env(env))
            }
        }
    }

    fn precompiles(&self) -> &Self::Precompiles {
        match self {
            Self::Eth(evm) => evm.precompiles(),
            Self::Op(evm) => evm.precompiles(),
            Self::ZKsync(evm) => evm.precompiles(),
        }
    }

    fn precompiles_mut(&mut self) -> &mut Self::Precompiles {
        match self {
            Self::Eth(evm) => evm.precompiles_mut(),
            Self::Op(evm) => evm.precompiles_mut(),
            Self::ZKsync(evm) => evm.precompiles_mut(),
        }
    }

    fn inspector(&self) -> &Self::Inspector {
        match self {
            Self::Eth(evm) => evm.inspector(),
            Self::Op(evm) => evm.inspector(),
            Self::ZKsync(evm) => evm.inspector(),
        }
    }

    fn inspector_mut(&mut self) -> &mut Self::Inspector {
        match self {
            Self::Eth(evm) => evm.inspector_mut(),
            Self::Op(evm) => evm.inspector_mut(),
            Self::ZKsync(evm) => evm.inspector_mut(),
        }
    }

    fn enable_inspector(&mut self) {
        match self {
            Self::Eth(evm) => evm.enable_inspector(),
            Self::Op(evm) => evm.enable_inspector(),
            Self::ZKsync(evm) => evm.enable_inspector(),
        }
    }

    fn disable_inspector(&mut self) {
        match self {
            Self::Eth(evm) => evm.disable_inspector(),
            Self::Op(evm) => evm.disable_inspector(),
            Self::ZKsync(evm) => evm.disable_inspector(),
        }
    }

    fn set_inspector_enabled(&mut self, enabled: bool) {
        match self {
            Self::Eth(evm) => evm.set_inspector_enabled(enabled),
            Self::Op(evm) => evm.set_inspector_enabled(enabled),
            Self::ZKsync(evm) => evm.set_inspector_enabled(enabled),
        }
    }

    fn into_env(self) -> EvmEnv<Self::Spec>
    where
        Self: Sized,
    {
        match self {
            Self::Eth(evm) => evm.into_env(),
            Self::Op(evm) => map_env(evm.into_env()),
            Self::ZKsync(evm) => map_zksync_env(evm.into_env()),
        }
    }

    fn transact(
        &mut self,
        tx: impl alloy_evm::IntoTxEnv<Self::Tx>,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        match self {
            Self::Eth(evm) => {
                let tx = match tx.into_tx_env() {
                    EitherTx::Eth(tx_env) => tx_env,
                    _ => panic!("invalid"),
                };
                let result = evm.transact(tx);
                self.map_either_evm_eth_result(result)
            }
            Self::Op(evm) => {
                let tx = match tx.into_tx_env() {
                    EitherTx::Op(tx_env) => tx_env,
                    _ => panic!("invalid"),
                };
                let result = evm.transact(tx);
                self.map_either_evm_op_result(result)
            }
            Self::ZKsync(evm) => {
                let tx = match tx.into_tx_env() {
                    EitherTx::ZKsync(tx_env) => tx_env,
                    _ => panic!("invalid"),
                };
                let result = evm.transact(tx);
                self.map_either_evm_zksync_result(result)
            }
        }
    }

    fn transact_commit(
        &mut self,
        tx: impl alloy_evm::IntoTxEnv<Self::Tx>,
    ) -> Result<ExecutionResult<Self::HaltReason>, Self::Error>
    where
        Self::DB: DatabaseCommit,
    {
        match self {
            Self::Eth(evm) => {
                let tx = match tx.into_tx_env() {
                    EitherTx::Eth(tx_env) => tx_env,
                    _ => panic!("invalid"),
                };
                let result = evm.transact_commit(tx);
                self.map_either_evm_exec_eth_result(result)
            }
            Self::Op(evm) => {
                let tx = match tx.into_tx_env() {
                    EitherTx::Op(tx_env) => tx_env,
                    _ => panic!("invalid"),
                };
                let result = evm.transact_commit(tx);
                self.map_either_evm_exec_op_result(result)
            }
            Self::ZKsync(evm) => {
                let tx = match tx.into_tx_env() {
                    EitherTx::ZKsync(tx_env) => tx_env,
                    _ => panic!("invalid"),
                };
                let result = evm.transact_commit(tx);
                self.map_either_evm_exec_zksync_result(result)
            }
        }
    }

    fn transact_raw(
        &mut self,
        tx: Self::Tx,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        match self {
            Self::Eth(evm) => {
                let tx = match tx.into_tx_env() {
                    EitherTx::Eth(tx_env) => tx_env,
                    _ => panic!("invalid"),
                };
                let result = evm.transact_raw(tx);
                self.map_either_evm_eth_result(result)
            }
            Self::Op(evm) => {
                let tx = match tx.into_tx_env() {
                    EitherTx::Op(tx_env) => tx_env,
                    _ => panic!("invalid"),
                };
                let result = evm.transact_raw(tx);
                self.map_either_evm_op_result(result)
            }
            Self::ZKsync(evm) => {
                let tx = match tx.into_tx_env() {
                    EitherTx::ZKsync(tx_env) => tx_env,
                    _ => panic!("invalid"),
                };
                let result = evm.transact_raw(tx);
                self.map_either_evm_zksync_result(result)
            }
        }
    }

    fn transact_system_call(
        &mut self,
        caller: Address,
        contract: Address,
        data: Bytes,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        match self {
            Self::Eth(evm) => {
                let result = evm.transact_system_call(caller, contract, data);
                self.map_either_evm_eth_result(result)
            }
            Self::Op(evm) => {
                let result = evm.transact_system_call(caller, contract, data);
                self.map_either_evm_op_result(result)
            }
            Self::ZKsync(evm) => {
                let result = evm.transact_system_call(caller, contract, data);
                self.map_either_evm_zksync_result(result)
            }
        }
    }
}

/// Maps [`EvmEnv<OpSpecId>`] to [`EvmEnv`].
fn map_env(env: EvmEnv<OpSpecId>) -> EvmEnv {
    let eth_spec_id = env.spec_id().into_eth_spec();
    let cfg = env.cfg_env.with_spec_and_mainnet_gas_params(eth_spec_id);
    EvmEnv { cfg_env: cfg, block_env: env.block_env }
}

fn map_zksync_env(env: EvmEnv<ZkSpecId>) -> EvmEnv {
    let eth_spec_id = env.spec_id().into_eth_spec();
    let cfg = env.cfg_env.with_spec_and_mainnet_gas_params(eth_spec_id);
    EvmEnv { cfg_env: cfg, block_env: env.block_env }
}
