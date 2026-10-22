//! A pack's `manifest.json`: who the pack is, and which version.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer};

/// The parts of a manifest the client uses.
#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub header: Header,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Header {
    /// Often a translation key, such as `resourcePack.vanilla.name`.
    pub name: String,
    /// Packs with the same UUID are versions of one pack: every vanilla
    /// resource pack shares one.
    pub uuid: String,
    pub version: Version,
    /// The oldest game version the pack is meant for.
    #[serde(default)]
    pub min_engine_version: Option<Version>,
}

/// A three-part version, ordered part by part: 1.21.90 is below 1.26.0.
///
/// Manifests write it as an array, `[1, 21, 90]`. Newer manifest formats
/// write a string, `"1.21.90"`, which is accepted too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub [u32; 3]);

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [major, minor, patch] = self.0;
        write!(f, "{major}.{minor}.{patch}")
    }
}

impl FromStr for Version {
    type Err = String;

    /// Parses `"1.26.51"`. Missing parts are 0, so `"1.14"` is 1.14.0.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = [0u32; 3];
        for (i, part) in text.split('.').enumerate() {
            let slot = parts
                .get_mut(i)
                .ok_or_else(|| format!("`{text}` has more than three parts"))?;
            *slot = part
                .parse()
                .map_err(|_| format!("`{text}` isn't a version: `{part}` isn't a number"))?;
        }
        Ok(Self(parts))
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// The two ways a manifest writes a version. `untagged` tries each
        /// variant in order and keeps the first that fits.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Written {
            Parts([u32; 3]),
            Text(String),
        }
        match Written::deserialize(deserializer)? {
            Written::Parts(parts) => Ok(Self(parts)),
            Written::Text(text) => text.parse().map_err(serde::de::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    #[test]
    fn reads_a_vanilla_style_manifest() {
        let text = br#"{"format_version":2,"header":{"description":"d","name":"resourcePack.vanilla.name",
            "uuid":"0575c61f-a5da-4b7f-9961-ffda2908861e","version":[1,21,90],
            "min_engine_version":[1,21,90],"pack_optimization_version":"0.1.0"},
            "modules":[],"capabilities":["pbr"]}"#;
        let manifest: Manifest = json::parse(text).unwrap();
        assert_eq!(manifest.header.uuid, "0575c61f-a5da-4b7f-9961-ffda2908861e");
        assert_eq!(manifest.header.version, Version([1, 21, 90]));
        assert_eq!(manifest.header.min_engine_version, Some(Version([1, 21, 90])));
    }

    #[test]
    fn versions_parse_from_strings_and_order_part_by_part() {
        let manifest: Manifest =
            json::parse(br#"{"header":{"name":"n","uuid":"u","version":"2.0.1"}}"#).unwrap();
        assert_eq!(manifest.header.version, Version([2, 0, 1]));
        assert_eq!(manifest.header.min_engine_version, None);

        assert_eq!("1.26.51".parse(), Ok(Version([1, 26, 51])));
        assert_eq!("1.14".parse(), Ok(Version([1, 14, 0])));
        assert!("1.2.3.4".parse::<Version>().is_err());
        assert!("1.x".parse::<Version>().is_err());
        assert!(Version([1, 21, 90]) < Version([1, 26, 0]));
        assert!(Version([0, 0, 1]) < Version([1, 14, 0]));
        assert_eq!(Version([1, 26, 51]).to_string(), "1.26.51");
    }
}
