use super::*;

/// Implements conversion from `BridgeGroup` to a slice of permission strings.
impl BridgeGroup {
    /// Returns the list of permission strings for this group.
    ///
    /// # Returns
    ///
    /// - `&[&str]`: The permission strings belonging to this group.
    pub(crate) fn permissions(&self) -> &[&str] {
        match self {
            BridgeGroup::Camera => BRIDGE_GROUP_CAMERA,
            BridgeGroup::Microphone => BRIDGE_GROUP_MICROPHONE,
            BridgeGroup::Location => BRIDGE_GROUP_LOCATION,
            BridgeGroup::Storage => BRIDGE_GROUP_STORAGE,
            BridgeGroup::Bluetooth => BRIDGE_GROUP_BLUETOOTH,
            BridgeGroup::Contacts => BRIDGE_GROUP_CONTACTS,
            BridgeGroup::Calendar => BRIDGE_GROUP_CALENDAR,
            BridgeGroup::Phone => BRIDGE_GROUP_PHONE,
            BridgeGroup::Sms => BRIDGE_GROUP_SMS,
            BridgeGroup::Sensors => BRIDGE_GROUP_SENSORS,
            BridgeGroup::Nfc => BRIDGE_GROUP_NFC,
            BridgeGroup::Notifications => BRIDGE_GROUP_NOTIFICATIONS,
            BridgeGroup::All => BRIDGE_DANGEROUS_PERMISSIONS,
        }
    }
}

/// Implements `std::fmt::Display` for `BridgeGroup`.
impl std::fmt::Display for BridgeGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name: &str = match self {
            BridgeGroup::Camera => "camera",
            BridgeGroup::Microphone => "microphone",
            BridgeGroup::Location => "location",
            BridgeGroup::Storage => "storage",
            BridgeGroup::Bluetooth => "bluetooth",
            BridgeGroup::Contacts => "contacts",
            BridgeGroup::Calendar => "calendar",
            BridgeGroup::Phone => "phone",
            BridgeGroup::Sms => "sms",
            BridgeGroup::Sensors => "sensors",
            BridgeGroup::Nfc => "nfc",
            BridgeGroup::Notifications => "notifications",
            BridgeGroup::All => "all",
        };
        write!(f, "{name}")
    }
}

/// Implements `std::str::FromStr` for `BridgeGroup` to parse from string input.
impl std::str::FromStr for BridgeGroup {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "camera" => Ok(BridgeGroup::Camera),
            "microphone" | "audio" => Ok(BridgeGroup::Microphone),
            "location" => Ok(BridgeGroup::Location),
            "storage" => Ok(BridgeGroup::Storage),
            "bluetooth" => Ok(BridgeGroup::Bluetooth),
            "contacts" => Ok(BridgeGroup::Contacts),
            "calendar" => Ok(BridgeGroup::Calendar),
            "phone" => Ok(BridgeGroup::Phone),
            "sms" => Ok(BridgeGroup::Sms),
            "sensors" => Ok(BridgeGroup::Sensors),
            "nfc" => Ok(BridgeGroup::Nfc),
            "notifications" => Ok(BridgeGroup::Notifications),
            "all" => Ok(BridgeGroup::All),
            other => Err(format!("Unknown bridge group: {other}")),
        }
    }
}
