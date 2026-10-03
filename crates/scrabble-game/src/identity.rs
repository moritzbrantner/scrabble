//! Numeric authority identifiers use decimal strings on the wire, avoiding JavaScript rounding.
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractError(pub String);

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ContractError {}

macro_rules! identifier {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
        )]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(u64);
        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }
            pub const fn get(self) -> u64 {
                self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = ContractError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                let number = value
                    .parse::<u64>()
                    .map_err(|_| ContractError("invalid identifier".into()))?;
                if number.to_string() != value {
                    return Err(ContractError(
                        "identifier must be canonical unsigned decimal".into(),
                    ));
                }
                Ok(Self(number))
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0.to_string()
            }
        }
    };
}
identifier!(GameId);
identifier!(PlayerId);
identifier!(TileId);
identifier!(TurnId);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct ProtocolVersion;
impl TryFrom<u16> for ProtocolVersion {
    type Error = ContractError;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value == 1 {
            Ok(Self)
        } else {
            Err(ContractError("unsupported protocol version".into()))
        }
    }
}
impl From<ProtocolVersion> for u16 {
    fn from(_: ProtocolVersion) -> Self {
        1
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawCoordinate", into = "RawCoordinate")]
pub struct Coordinate {
    row: u8,
    column: u8,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCoordinate {
    row: u8,
    column: u8,
}
impl Coordinate {
    pub fn new(row: u8, column: u8) -> Result<Self, ContractError> {
        // v1 supports square boards up to 15 by 15. The configured layout is checked by the engine.
        if row >= 15 || column >= 15 {
            return Err(ContractError("coordinate outside v1 board bounds".into()));
        }
        Ok(Self { row, column })
    }
    pub const fn row(self) -> u8 {
        self.row
    }
    pub const fn column(self) -> u8 {
        self.column
    }
}
impl TryFrom<RawCoordinate> for Coordinate {
    type Error = ContractError;
    fn try_from(value: RawCoordinate) -> Result<Self, Self::Error> {
        Self::new(value.row, value.column)
    }
}
impl From<Coordinate> for RawCoordinate {
    fn from(value: Coordinate) -> Self {
        Self {
            row: value.row,
            column: value.column,
        }
    }
}
