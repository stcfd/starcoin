// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::account_address::AccountAddress;
use crate::sign_message::SigningMessage;
use crate::transaction::{RawUserTransaction, SignedUserTransaction};
use anyhow::{ensure, Error, Result};
#[cfg(any(test, feature = "fuzzing"))]
use proptest_derive::Arbitrary;
use rand::{rngs::OsRng, Rng};
use serde::{Deserialize, Serialize};
use starcoin_crypto::ed25519::{
    Ed25519PrivateKey, ED25519_PRIVATE_KEY_LENGTH, ED25519_PUBLIC_KEY_LENGTH,
    ED25519_SIGNATURE_LENGTH,
};
use starcoin_crypto::multi_ed25519::multi_shard::{
    MultiEd25519KeyShard, MultiEd25519SignatureShard,
};
use starcoin_crypto::{
    derive::{DeserializeKey, SerializeKey},
    ed25519::{Ed25519PublicKey, Ed25519Signature},
    hash::{CryptoHash, CryptoHasher},
    multi_ed25519::{MultiEd25519PublicKey, MultiEd25519Signature},
    traits::Signature,
    CryptoMaterialError, HashValue, PrivateKey, SigningKey, ValidCryptoMaterial,
    ValidCryptoMaterialStringExt,
};
use std::{convert::TryFrom, fmt, str::FromStr};

/// A `TransactionAuthenticator` is an an abstraction of a signature scheme. It must know:
/// (1) How to check its signature against a message and public key
/// (2) How to convert its public key into an `AuthenticationKeyPreimage` structured as
/// (public_key | signaure_scheme_id).
/// Each on-chain `DiemAccount` must store an `AuthenticationKey` (computed via a sha3 hash of an
/// `AuthenticationKeyPreimage`).
/// Each transaction submitted to the Diem blockchain contains a `TransactionAuthenticator`. During
/// transaction execution, the executor will check if the `TransactionAuthenticator`'s signature on
/// the transaction hash is well-formed (1) and whether the sha3 hash of the
/// `TransactionAuthenticator`'s `AuthenticationKeyPreimage` matches the `AuthenticationKey` stored
/// under the transaction's sender account address (2).

// TODO: in the future, can tie these to the TransactionAuthenticator enum directly with https://github.com/rust-lang/rust/issues/60553
#[derive(Debug)]
#[repr(u8)]
pub enum Scheme {
    Ed25519 = 0,
    MultiEd25519 = 1,
    // ... add more schemes here
}

impl fmt::Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let display = match self {
            Scheme::Ed25519 => "Ed25519",
            Scheme::MultiEd25519 => "MultiEd25519",
        };
        write!(f, "Scheme::{}", display)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum TransactionAuthenticator {
    /// Single signature
    Ed25519 {
        public_key: Ed25519PublicKey,
        signature: Ed25519Signature,
    },
    /// K-of-N multisignature
    MultiEd25519 {
        public_key: MultiEd25519PublicKey,
        signature: MultiEd25519Signature,
    },
    // ... add more schemes here
}

impl TransactionAuthenticator {
    /// Unique identifier for the signature scheme
    pub fn scheme(&self) -> Scheme {
        match self {
            Self::Ed25519 { .. } => Scheme::Ed25519,
            Self::MultiEd25519 { .. } => Scheme::MultiEd25519,
        }
    }

    /// Create a single-signature ed25519 authenticator
    pub fn ed25519(public_key: Ed25519PublicKey, signature: Ed25519Signature) -> Self {
        Self::Ed25519 {
            public_key,
            signature,
        }
    }

    /// Create a multisignature ed25519 authenticator
    pub fn multi_ed25519(
        public_key: MultiEd25519PublicKey,
        signature: MultiEd25519Signature,
    ) -> Self {
        Self::MultiEd25519 {
            public_key,
            signature,
        }
    }

    /// Return Ok if the authenticator's public key matches its signature, Err otherwise
    pub fn verify<T: Serialize + CryptoHash>(&self, message: &T) -> Result<()> {
        match self {
            Self::Ed25519 {
                public_key,
                signature,
            } => signature.verify(message, public_key),
            Self::MultiEd25519 {
                public_key,
                signature,
            } => signature.verify(message, public_key),
        }
    }

    /// Return the raw bytes of `self.public_key`
    pub fn public_key_bytes(&self) -> Vec<u8> {
        match self {
            Self::Ed25519 { public_key, .. } => public_key.to_bytes().to_vec(),
            Self::MultiEd25519 { public_key, .. } => public_key.to_bytes().to_vec(),
        }
    }

    pub fn public_key(&self) -> AccountPublicKey {
        match self {
            Self::Ed25519 { public_key, .. } => AccountPublicKey::Single(public_key.clone()),
            Self::MultiEd25519 { public_key, .. } => AccountPublicKey::Multi(public_key.clone()),
        }
    }

    /// Return the raw bytes of `self.signature`
    pub fn signature_bytes(&self) -> Vec<u8> {
        match self {
            Self::Ed25519 { signature, .. } => signature.to_bytes().to_vec(),
            Self::MultiEd25519 { signature, .. } => signature.to_bytes().to_vec(),
        }
    }

    /// Return an authentication key preimage derived from `self`'s public key and scheme id
    pub fn authentication_key_preimage(&self) -> AuthenticationKeyPreimage {
        AuthenticationKeyPreimage::new(self.public_key_bytes(), self.scheme())
    }

    /// Return an authentication key derived from `self`'s public key and scheme id
    pub fn authentication_key(&self) -> AuthenticationKey {
        AuthenticationKey::from_preimage(&self.authentication_key_preimage())
    }
}

/// A struct that represents an account authentication key. An account's address is the last 16
/// bytes of authentication key used to create it
#[derive(
    Clone,
    Copy,
    CryptoHasher,
    Debug,
    DeserializeKey,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    SerializeKey,
)]
#[cfg_attr(any(test, feature = "fuzzing"), derive(Arbitrary))]
pub struct AuthenticationKey([u8; AuthenticationKey::LENGTH]);

impl AuthenticationKey {
    /// Create an authentication key from `bytes`
    pub const fn new(bytes: [u8; Self::LENGTH]) -> Self {
        Self(bytes)
    }

    /// The number of bytes in an authentication key.
    pub const LENGTH: usize = 32;

    /// Create an authentication key from a preimage by taking its sha3 hash
    pub fn from_preimage(preimage: &AuthenticationKeyPreimage) -> AuthenticationKey {
        AuthenticationKey::new(*HashValue::sha3_256_of(&preimage.0).as_ref())
    }

    /// Create an authentication key from an Ed25519 public key
    pub fn ed25519(public_key: &Ed25519PublicKey) -> AuthenticationKey {
        Self::from_preimage(&AuthenticationKeyPreimage::ed25519(public_key))
    }

    /// Create an authentication key from a MultiEd25519 public key
    pub fn multi_ed25519(public_key: &MultiEd25519PublicKey) -> Self {
        Self::from_preimage(&AuthenticationKeyPreimage::multi_ed25519(public_key))
    }

    /// Return an address derived from the last `AccountAddress::LENGTH` bytes of this
    /// authentication key.
    pub fn derived_address(&self) -> AccountAddress {
        // keep only last 16 bytes
        let mut array = [0u8; AccountAddress::LENGTH];
        array.copy_from_slice(&self.0[Self::LENGTH - AccountAddress::LENGTH..]);
        AccountAddress::new(array)
    }

    /// Return the first AccountAddress::LENGTH bytes of this authentication key
    pub fn prefix(&self) -> [u8; AccountAddress::LENGTH] {
        let mut array = [0u8; AccountAddress::LENGTH];
        array.copy_from_slice(&self.0[..AccountAddress::LENGTH]);
        array
    }

    /// Construct a vector from this authentication key
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    /// Create a random authentication key. For testing only
    pub fn random() -> Self {
        let mut rng = OsRng;
        let buf: [u8; Self::LENGTH] = rng.gen();
        AuthenticationKey::new(buf)
    }
}

impl ValidCryptoMaterial for AuthenticationKey {
    fn to_bytes(&self) -> Vec<u8> {
        self.to_vec()
    }
}

/// A value that can be hashed to produce an authentication key
pub struct AuthenticationKeyPreimage(Vec<u8>);

impl AuthenticationKeyPreimage {
    /// Return bytes for (public_key | scheme_id)
    fn new(mut public_key_bytes: Vec<u8>, scheme: Scheme) -> Self {
        public_key_bytes.push(scheme as u8);
        Self(public_key_bytes)
    }

    /// Construct a preimage from an Ed25519 public key
    pub fn ed25519(public_key: &Ed25519PublicKey) -> AuthenticationKeyPreimage {
        Self::new(public_key.to_bytes().to_vec(), Scheme::Ed25519)
    }

    /// Construct a preimage from a MultiEd25519 public key
    pub fn multi_ed25519(public_key: &MultiEd25519PublicKey) -> AuthenticationKeyPreimage {
        Self::new(public_key.to_bytes(), Scheme::MultiEd25519)
    }

    /// Construct a vector from this authentication key
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl fmt::Display for TransactionAuthenticator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TransactionAuthenticator[scheme id: {:?}, public key: {}, signature: {}]",
            self.scheme(),
            hex::encode(&self.public_key_bytes()),
            hex::encode(&self.signature_bytes())
        )
    }
}

impl TryFrom<&[u8]> for AuthenticationKey {
    type Error = CryptoMaterialError;

    fn try_from(bytes: &[u8]) -> std::result::Result<AuthenticationKey, CryptoMaterialError> {
        if bytes.len() != Self::LENGTH {
            return Err(CryptoMaterialError::WrongLengthError);
        }
        let mut addr = [0u8; Self::LENGTH];
        addr.copy_from_slice(bytes);
        Ok(AuthenticationKey(addr))
    }
}

impl TryFrom<Vec<u8>> for AuthenticationKey {
    type Error = CryptoMaterialError;

    fn try_from(bytes: Vec<u8>) -> std::result::Result<AuthenticationKey, CryptoMaterialError> {
        AuthenticationKey::try_from(&bytes[..])
    }
}

impl FromStr for AuthenticationKey {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        ensure!(
            !s.is_empty(),
            "authentication key string should not be empty.",
        );
        Ok(AuthenticationKey::from_encoded_string(s)?)
    }
}

impl AsRef<[u8]> for AuthenticationKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::LowerHex for AuthenticationKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(&self.0))
    }
}

impl fmt::Display for AuthenticationKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
        // Forward to the LowerHex impl with a "0x" prepended (the # flag).
        write!(f, "0x{:#x}", self)
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, DeserializeKey, SerializeKey)]
pub enum AccountPublicKey {
    Single(Ed25519PublicKey),
    Multi(MultiEd25519PublicKey),
}

#[derive(Eq, PartialEq, Debug, DeserializeKey, SerializeKey)]
pub enum AccountPrivateKey {
    Single(Ed25519PrivateKey),
    Multi(MultiEd25519KeyShard),
}

#[derive(Clone, Debug, Hash, PartialEq, DeserializeKey, SerializeKey, Eq)]
pub enum AccountSignature {
    Single(Ed25519PublicKey, Ed25519Signature),
    Multi(MultiEd25519PublicKey, MultiEd25519SignatureShard),
}
impl ValidCryptoMaterial for AccountSignature {
    fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Single(public_key, signature) => {
                let mut bytes = public_key.to_bytes().to_vec();
                bytes.extend(signature.to_bytes().to_vec());
                bytes
            }
            Self::Multi(multi_key, multi_signed_shard) => {
                let mut bytes = multi_key.to_bytes().to_vec();
                bytes.extend(multi_signed_shard.to_bytes().to_vec());
                bytes
            }
        }
    }
}
impl ValidCryptoMaterial for AccountPublicKey {
    fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Single(key) => key.to_bytes().to_vec(),
            Self::Multi(key) => key.to_bytes(),
        }
    }
}

impl AccountPublicKey {
    pub fn derived_address(&self) -> AccountAddress {
        self.authentication_key().derived_address()
    }
    /// Return an authentication key preimage derived from `self`'s public key and scheme id
    pub fn authentication_key_preimage(&self) -> AuthenticationKeyPreimage {
        match self {
            Self::Single(p) => AuthenticationKeyPreimage::ed25519(p),
            Self::Multi(p) => AuthenticationKeyPreimage::multi_ed25519(p),
        }
    }

    /// Return an authentication key derived from `self`'s public key and scheme id
    pub fn authentication_key(&self) -> AuthenticationKey {
        AuthenticationKey::from_preimage(&self.authentication_key_preimage())
    }

    /// Return the raw bytes of `self.public_key`
    pub fn public_key_bytes(&self) -> Vec<u8> {
        match self {
            Self::Single(public_key) => public_key.to_bytes().to_vec(),
            Self::Multi(public_key) => public_key.to_bytes().to_vec(),
        }
    }

    /// Unique identifier for the signature scheme
    pub fn scheme(&self) -> Scheme {
        match self {
            Self::Single { .. } => Scheme::Ed25519,
            Self::Multi { .. } => Scheme::MultiEd25519,
        }
    }

    pub fn as_single(&self) -> Option<Ed25519PublicKey> {
        match self {
            Self::Single(key) => Some(key.clone()),
            _ => None,
        }
    }

    pub fn as_multi(&self) -> Option<MultiEd25519PublicKey> {
        match self {
            Self::Multi(key) => Some(key.clone()),
            _ => None,
        }
    }
}

impl TryFrom<&[u8]> for AccountPublicKey {
    type Error = CryptoMaterialError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() == ED25519_PUBLIC_KEY_LENGTH {
            Ed25519PublicKey::try_from(value).map(Self::Single)
        } else {
            MultiEd25519PublicKey::try_from(value).map(Self::Multi)
        }
    }
}

#[allow(clippy::from_over_into)]
impl Into<AccountPublicKey> for Ed25519PublicKey {
    fn into(self) -> AccountPublicKey {
        AccountPublicKey::Single(self)
    }
}

#[allow(clippy::from_over_into)]
impl Into<AccountPublicKey> for MultiEd25519PublicKey {
    fn into(self) -> AccountPublicKey {
        AccountPublicKey::Multi(self)
    }
}

impl ValidCryptoMaterial for AccountPrivateKey {
    fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Single(key) => key.to_bytes().to_vec(),
            Self::Multi(key) => key.to_bytes(),
        }
    }
}

impl AccountPrivateKey {
    pub fn public_key(&self) -> AccountPublicKey {
        match self {
            Self::Single(key) => AccountPublicKey::Single(key.public_key()),
            Self::Multi(key) => AccountPublicKey::Multi(key.public_key()),
        }
    }

    pub fn sign<T: CryptoHash + Serialize>(&self, message: &T) -> AccountSignature {
        match self {
            Self::Single(key) => AccountSignature::Single(key.public_key(), key.sign(message)),
            Self::Multi(key) => AccountSignature::Multi(key.public_key(), key.sign(message)),
        }
    }

    pub fn sign_message(&self, message: SigningMessage) -> AccountSignature {
        self.sign(&message)
    }
}

#[allow(clippy::from_over_into)]
impl Into<AccountPrivateKey> for Ed25519PrivateKey {
    fn into(self) -> AccountPrivateKey {
        AccountPrivateKey::Single(self)
    }
}

#[allow(clippy::from_over_into)]
impl Into<AccountPrivateKey> for MultiEd25519KeyShard {
    fn into(self) -> AccountPrivateKey {
        AccountPrivateKey::Multi(self)
    }
}

impl TryFrom<&[u8]> for AccountPrivateKey {
    type Error = CryptoMaterialError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() == ED25519_PRIVATE_KEY_LENGTH {
            Ed25519PrivateKey::try_from(value).map(Self::Single)
        } else {
            MultiEd25519KeyShard::try_from(value).map(Self::Multi)
        }
    }
}

impl TryFrom<&[u8]> for AccountSignature {
    type Error = CryptoMaterialError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let length = value.len();
        if length == ED25519_PUBLIC_KEY_LENGTH + ED25519_SIGNATURE_LENGTH {
            let public_key = Ed25519PublicKey::try_from(&value[..ED25519_PUBLIC_KEY_LENGTH])?;
            let signature = Ed25519Signature::try_from(&value[ED25519_PUBLIC_KEY_LENGTH..])?;
            Ok(Self::Single(public_key, signature))
        } else {
            // 1 is MultiEd25519PublicKey's threshold
            // 4 is  MultiEd25519Signature's bitmap
            // 1 is MultiEd25519SignatureShard's threshold
            let key_size =
                (length - 1 - 4 - 1) / (ED25519_PUBLIC_KEY_LENGTH + ED25519_SIGNATURE_LENGTH);
            let key_len = key_size * ED25519_PUBLIC_KEY_LENGTH + 1;
            let multi_public_key = MultiEd25519PublicKey::try_from(&value[..key_len])?;
            let multi_signature = MultiEd25519Signature::try_from(&value[key_len..length - 1])?;
            Ok(Self::Multi(
                multi_public_key,
                MultiEd25519SignatureShard::new(multi_signature, value[length]),
            ))
        }
    }
}

impl AccountSignature {
    pub fn build_transaction(self, raw_txn: RawUserTransaction) -> Result<SignedUserTransaction> {
        Ok(match self {
            Self::Single(public_key, signature) => {
                SignedUserTransaction::ed25519(raw_txn, public_key, signature)
            }
            Self::Multi(public_key, signature) => {
                if signature.is_enough() {
                    SignedUserTransaction::multi_ed25519(raw_txn, public_key, signature.into())
                } else {
                    anyhow::bail!(
                        "MultiEd25519SignatureShard do not have enough signatures, current: {}, threshold: {}",
                        signature.signatures().len(),
                        signature.threshold()
                    )
                }
            }
        })
    }

    pub fn verify<T: Serialize + CryptoHash>(&self, message: &T) -> Result<()> {
        match self {
            Self::Single(public_key, signature) => signature.verify(message, public_key),
            Self::Multi(public_key, signature) => signature.verify(message, public_key),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::transaction::authenticator::AuthenticationKey;
    use std::str::FromStr;

    #[test]
    fn test_from_str_should_not_panic_by_given_empty_string() {
        assert!(AuthenticationKey::from_str("").is_err());
    }
}
