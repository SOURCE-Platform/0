//! Credential-vault wire types and secret-free verification (spec v0.4
//! §1.1): TLV, the registry codec and structural chain verification,
//! backup objects, index, signed manifest and checkpoint, the revision
//! identity, `ProviderRequest`, the state commitment and handle
//! normalization. Shared by the helper, `vault-provider-core` and tests;
//! moved out of `vault-helper` without behaviour change.

pub mod b64;
pub mod backup;
pub mod handle;
pub mod header;
pub mod crypto;
pub mod errors;
pub mod registry;
pub mod request;
pub mod rev;
pub mod state;
