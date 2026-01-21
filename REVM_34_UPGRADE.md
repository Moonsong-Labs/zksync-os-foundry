# REVM 34.0.0 Upgrade Guide for zksync-os-foundry

## Overview

This document details the changes required to upgrade zksync-os-foundry from revm 33.1.0 to 34.0.0. The upgrade follows the upstream Foundry PR [#13130](https://github.com/foundry-rs/foundry/pull/13130) and includes additional zksync-specific changes.

## Version Changes

### Cargo.toml Updates

| Package | Previous Version | New Version |
|---------|-----------------|-------------|
| revm | 33.1.0 | 34.0.0 |
| revm-inspectors | 0.33.2 | 0.34.0 |
| op-revm | 14.1.0 | 15.0.0 |
| alloy-evm | 0.25.2 | 0.26.3 |
| alloy-op-evm | 0.25.2 | 0.26.3 |
| foundry-fork-db | 0.21 | 0.22 |
| zksync-os-revm | git branch revm-33.1.0 | local path (revm-34.0.0 branch) |

## Files Modified

17 files were modified in total:

### Cargo Files
- `Cargo.toml` - Dependency version updates
- `Cargo.lock` - Lock file updates

### Cheatcodes Crate
- `crates/cheatcodes/src/fs.rs` - CreateInputs constructor change
- `crates/cheatcodes/src/inspector.rs` - CreateInputs field access via methods
- `crates/cheatcodes/src/inspector/utils.rs` - CommonCreateInput trait implementation

### EVM Core Crate
- `crates/evm/core/src/backend/mod.rs` - JournaledAccountTr and StateLoad wrapper
- `crates/evm/core/src/either_evm.rs` - Spec conversion with gas params
- `crates/evm/core/src/evm.rs` - CreateInputs API and validate_initial_tx_gas signature

### EVM Inspectors
- `crates/evm/evm/src/inspectors/custom_printer.rs` - CreateInputs field access
- `crates/evm/evm/src/inspectors/stack.rs` - CreateInputs field access

### Anvil Backend
- `crates/anvil/src/eth/backend/db.rs` - AddressMap type, AccountInfo.account_id
- `crates/anvil/src/eth/backend/executor.rs` - Spec conversion method
- `crates/anvil/src/eth/backend/genesis.rs` - AccountInfo.account_id
- `crates/anvil/src/eth/backend/mem/fork_db.rs` - AddressMap type
- `crates/anvil/src/eth/backend/mem/in_memory_db.rs` - AddressMap type
- `crates/anvil/src/eth/backend/mem/mod.rs` - AddressMap import
- `crates/anvil/src/eth/backend/mem/state.rs` - AddressMap type in function signatures

## API Changes in REVM 34

### 1. CreateInputs Fields are Now Private

**Breaking Change**: In revm 34, `CreateInputs` struct fields are private and must be accessed via methods.

#### Previous Pattern (v33.1.0)
```rust
let caller = inputs.caller;
let scheme = inputs.scheme;
let value = inputs.value;
let init_code = inputs.init_code.clone();
let gas_limit = inputs.gas_limit;

let create = CreateInputs {
    caller,
    scheme,
    value,
    init_code,
    gas_limit,
};
```

#### New Pattern (v34.0.0)
```rust
// Field access via methods
let caller = inputs.caller();
let scheme = inputs.scheme();
let value = inputs.value();
let init_code = inputs.init_code().clone();
let gas_limit = inputs.gas_limit();

// Construction via new()
let create = CreateInputs::new(caller, scheme, value, init_code, gas_limit);
```

**Files Affected**:
- `crates/cheatcodes/src/fs.rs`
- `crates/cheatcodes/src/inspector.rs`
- `crates/cheatcodes/src/inspector/utils.rs`
- `crates/evm/core/src/evm.rs`
- `crates/evm/evm/src/inspectors/custom_printer.rs`
- `crates/evm/evm/src/inspectors/stack.rs`

### 2. CreateInputs Setters

**Breaking Change**: Setter method names changed.

| Old Method | New Method |
|-----------|------------|
| N/A (direct field access) | `set_call(caller)` |
| N/A | `set_scheme(scheme)` |
| N/A | `set_value(value)` |
| N/A | `set_init_code(code)` |
| N/A | `set_gas_limit(limit)` |

**Example in CommonCreateInput trait**:
```rust
impl CommonCreateInput for CreateInputs {
    fn caller(&self) -> Address { CreateInputs::caller(self) }
    fn gas_limit(&self) -> u64 { CreateInputs::gas_limit(self) }
    fn value(&self) -> U256 { CreateInputs::value(self) }
    fn init_code(&self) -> Bytes { CreateInputs::init_code(self).clone() }
    fn scheme(&self) -> Option<CreateScheme> { Some(CreateInputs::scheme(self)) }
    fn set_caller(&mut self, caller: Address) { self.set_call(caller); }
    fn set_gas_limit(&mut self, limit: u64) { self.set_gas_limit(limit); }
    fn set_value(&mut self, value: U256) { self.set_value(value); }
    fn set_init_code(&mut self, init_code: Bytes) { self.set_init_code(init_code); }
}
```

### 3. JournaledAccountTr Trait Import Required

**Breaking Change**: Account methods like `set_code()` and `set_balance()` now require the trait import.

#### Required Import
```rust
use revm::context_interface::journaled_state::account::JournaledAccountTr;
```

#### StateLoad Wrapper Access
Account data is now wrapped in `StateLoad`, requiring `.data` accessor:

```rust
// Previous
state_acc.set_code(code);
state_acc.set_balance(balance);

// New
state_acc.data.set_code(code);
state_acc.data.set_balance(balance);
```

**Files Affected**:
- `crates/evm/core/src/backend/mod.rs`

### 4. HashMap → AddressMap Type Change

**Breaking Change**: Account storage type changed from `HashMap<Address, DbAccount>` to `AddressMap<DbAccount>`.

`AddressMap` is a type alias for `HashMap` with a specific hasher (`FbBuildHasher<20>`):
```rust
pub type AddressMap<V> = HashMap<Address, V, FbBuildHasher<20>>;
```

#### Required Import
```rust
use alloy_primitives::map::AddressMap;
```

**Files Affected**:
- `crates/anvil/src/eth/backend/db.rs`
- `crates/anvil/src/eth/backend/mem/fork_db.rs`
- `crates/anvil/src/eth/backend/mem/in_memory_db.rs`
- `crates/anvil/src/eth/backend/mem/mod.rs`
- `crates/anvil/src/eth/backend/mem/state.rs`

### 5. AccountInfo.account_id New Field

**Breaking Change**: `AccountInfo` struct now has an `account_id` field.

#### Fix
Add `account_id: None` when constructing `AccountInfo`:

```rust
AccountInfo {
    nonce: nonce.unwrap_or_default(),
    balance,
    code_hash,
    code: Some(code),
    account_id: None,  // New field
}
```

**Files Affected**:
- `crates/anvil/src/eth/backend/db.rs`
- `crates/anvil/src/eth/backend/genesis.rs`

### 6. CfgEnv Spec Conversion

**Breaking Change**: Setting spec on `CfgEnv` now requires gas parameters.

#### Previous Pattern
```rust
cfg.with_spec(spec)
```

#### New Pattern
```rust
cfg.with_spec_and_mainnet_gas_params(spec)
```

This is needed when converting between spec types (e.g., `OpSpecId` to Ethereum `SpecId`).

**Files Affected**:
- `crates/evm/core/src/either_evm.rs`
- `crates/anvil/src/eth/backend/executor.rs`

### 7. validate_initial_tx_gas Signature Change

**Breaking Change**: The parameter type changed from `&Self::Evm` to `&mut Self::Evm`.

```rust
// Previous
fn validate_initial_tx_gas(&self, evm: &Self::Evm) -> Result<(), Self::Error>

// New
fn validate_initial_tx_gas(&self, evm: &mut Self::Evm) -> Result<(), Self::Error>
```

**Files Affected**:
- `crates/evm/core/src/evm.rs`

## Detailed Changes by File

### crates/cheatcodes/src/fs.rs
- Changed `CreateInputs` construction from struct literal to `CreateInputs::new()` constructor

### crates/cheatcodes/src/inspector.rs
- Changed `call.caller` → `call.caller()`
- Changed `call.scheme` → `call.scheme()`
- Changed `inputs.scheme` → `inputs.scheme()`

### crates/cheatcodes/src/inspector/utils.rs
- Implemented `CommonCreateInput` trait with new method-based API
- Used `self.set_call(caller)` instead of direct field assignment

### crates/evm/core/src/backend/mod.rs
- Added `JournaledAccountTr` import
- Changed `state_acc.set_code()` → `state_acc.data.set_code()`
- Changed `state_acc.set_balance()` → `state_acc.data.set_balance()`

### crates/evm/core/src/either_evm.rs
- Created `map_env()` function for `OpSpecId` → `SpecId` conversion
- Used `with_spec_and_mainnet_gas_params()` for proper type conversion

### crates/evm/core/src/evm.rs
- Changed all `CreateInputs` field access to method calls
- Changed `validate_initial_tx_gas` signature to use `&mut Self::Evm`

### crates/evm/evm/src/inspectors/custom_printer.rs
- Changed field access: `create.caller` → `create.caller()`, etc.

### crates/evm/evm/src/inspectors/stack.rs
- Changed all `CreateInputs` field access to method calls
- Used `create.scheme()`, `create.caller()`, `create.init_code()`, etc.

### crates/anvil/src/eth/backend/db.rs
- Added `AddressMap` import
- Changed `MaybeFullDatabase::maybe_as_full_db()` return type to `AddressMap<DbAccount>`
- Added `account_id: None` to `AccountInfo` structs

### crates/anvil/src/eth/backend/genesis.rs
- Added `account_id: None` to `AccountInfo` struct

### crates/anvil/src/eth/backend/executor.rs
- Changed `with_spec()` → `with_spec_and_mainnet_gas_params()`

### crates/anvil/src/eth/backend/mem/*.rs
- Updated all type signatures from `HashMap<Address, DbAccount>` to `AddressMap<DbAccount>`
- Added `AddressMap` import where needed

## How Upstream Foundry Handled the Upgrade

Based on [PR #13130](https://github.com/foundry-rs/foundry/pull/13130), upstream Foundry made identical changes:

1. **CreateInputs API** - Same method-based access pattern
2. **AddressMap type** - Same type alias usage
3. **AccountInfo.account_id** - Same `None` value for new accounts
4. **JournaledAccountTr** - Same trait import for account operations
5. **Spec conversion** - Same `with_spec_and_mainnet_gas_params()` usage

The zksync-os-foundry changes mirror upstream exactly for compatibility.

## zksync-os-revm Dependency

The `zksync-os-revm` crate was also updated to support revm 34.0.0. Key changes there:

1. **JournaledAccount access** via `.data` field
2. **Balance methods** now return references (`&U256`)
3. **Manual Default impl** for generic types
4. **Borrow checker** satisfaction with scoped blocks

See `/Users/juanrigada/msl/zksync-os-revm/REVM_34_UPGRADE.md` for full details.

## Migration Steps Applied

1. Updated `Cargo.toml` with new versions
2. Changed `zksync-os-revm` to local path reference
3. Fixed `CreateInputs` API changes in cheatcodes and inspectors
4. Added `JournaledAccountTr` trait import where needed
5. Changed `HashMap` → `AddressMap` in anvil backend
6. Added `account_id: None` to `AccountInfo` structs
7. Fixed spec conversion methods
8. Fixed `validate_initial_tx_gas` signature

## Testing

After applying changes:

```bash
# Build the workspace
cargo build

# Run tests
cargo test

# Check for warnings
cargo clippy
```

## Notes

- All changes compile successfully
- The upgrade is backwards compatible with existing functionality
- No behavior changes, only API surface changes
- Follow upstream Foundry for future revm upgrades

## Resources

- [Foundry PR #13130](https://github.com/foundry-rs/foundry/pull/13130) - Upstream upgrade reference
- [revm releases](https://github.com/bluealloy/revm/releases) - Release notes
- [zksync-os-revm REVM_34_UPGRADE.md](../zksync-os-revm/REVM_34_UPGRADE.md) - Companion documentation
