use alloy_evm::EvmEnv;
use foundry_evm::{
    EnvMut,
    core::{AsEnvMut, either_evm::EitherTx},
};
use foundry_evm_networks::NetworkConfigs;

/// Helper container type for [`EvmEnv`] and [`OpTransaction<TxEnd>`].
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub evm_env: EvmEnv,
    pub tx: EitherTx,
    pub networks: NetworkConfigs,
}

/// Helper container type for [`EvmEnv`] and [`OpTransaction<TxEnv>`].
impl Env {
    pub fn new(evm_env: EvmEnv, tx: EitherTx, networks: NetworkConfigs) -> Self {
        Self { evm_env, tx, networks }
    }
}

impl AsEnvMut for Env {
    fn as_env_mut(&mut self) -> EnvMut<'_> {
        let tx = match &mut self.tx {
            EitherTx::Eth(tx_env) => tx_env,
            EitherTx::Op(op_transaction) => &mut op_transaction.base,
            EitherTx::ZKsync(zksync_tx) => &mut zksync_tx.base,
        };
        EnvMut { block: &mut self.evm_env.block_env, cfg: &mut self.evm_env.cfg_env, tx }
    }
}
