// @generated
impl serde::Serialize for BackgroundQuota {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.max_concurrent_background != 0 {
            len += 1;
        }
        if self.max_daily_server_turns != 0 {
            len += 1;
        }
        if self.require_user_consent {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.policy.BackgroundQuota", len)?;
        if self.max_concurrent_background != 0 {
            struct_ser.serialize_field("maxConcurrentBackground", &self.max_concurrent_background)?;
        }
        if self.max_daily_server_turns != 0 {
            struct_ser.serialize_field("maxDailyServerTurns", &self.max_daily_server_turns)?;
        }
        if self.require_user_consent {
            struct_ser.serialize_field("requireUserConsent", &self.require_user_consent)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for BackgroundQuota {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "max_concurrent_background",
            "maxConcurrentBackground",
            "max_daily_server_turns",
            "maxDailyServerTurns",
            "require_user_consent",
            "requireUserConsent",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            MaxConcurrentBackground,
            MaxDailyServerTurns,
            RequireUserConsent,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "maxConcurrentBackground" | "max_concurrent_background" => Ok(GeneratedField::MaxConcurrentBackground),
                            "maxDailyServerTurns" | "max_daily_server_turns" => Ok(GeneratedField::MaxDailyServerTurns),
                            "requireUserConsent" | "require_user_consent" => Ok(GeneratedField::RequireUserConsent),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = BackgroundQuota;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.policy.BackgroundQuota")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<BackgroundQuota, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut max_concurrent_background__ = None;
                let mut max_daily_server_turns__ = None;
                let mut require_user_consent__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::MaxConcurrentBackground => {
                            if max_concurrent_background__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxConcurrentBackground"));
                            }
                            max_concurrent_background__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::MaxDailyServerTurns => {
                            if max_daily_server_turns__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxDailyServerTurns"));
                            }
                            max_daily_server_turns__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::RequireUserConsent => {
                            if require_user_consent__.is_some() {
                                return Err(serde::de::Error::duplicate_field("requireUserConsent"));
                            }
                            require_user_consent__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(BackgroundQuota {
                    max_concurrent_background: max_concurrent_background__.unwrap_or_default(),
                    max_daily_server_turns: max_daily_server_turns__.unwrap_or_default(),
                    require_user_consent: require_user_consent__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.policy.BackgroundQuota", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for CuaPolicy {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.enabled {
            len += 1;
        }
        if !self.allowed_apps.is_empty() {
            len += 1;
        }
        if !self.denied_apps.is_empty() {
            len += 1;
        }
        if self.screenshot_redaction {
            len += 1;
        }
        if self.require_confirmation_destructive {
            len += 1;
        }
        if self.max_actions_per_minute != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.policy.CuaPolicy", len)?;
        if self.enabled {
            struct_ser.serialize_field("enabled", &self.enabled)?;
        }
        if !self.allowed_apps.is_empty() {
            struct_ser.serialize_field("allowedApps", &self.allowed_apps)?;
        }
        if !self.denied_apps.is_empty() {
            struct_ser.serialize_field("deniedApps", &self.denied_apps)?;
        }
        if self.screenshot_redaction {
            struct_ser.serialize_field("screenshotRedaction", &self.screenshot_redaction)?;
        }
        if self.require_confirmation_destructive {
            struct_ser.serialize_field("requireConfirmationDestructive", &self.require_confirmation_destructive)?;
        }
        if self.max_actions_per_minute != 0 {
            struct_ser.serialize_field("maxActionsPerMinute", &self.max_actions_per_minute)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for CuaPolicy {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "enabled",
            "allowed_apps",
            "allowedApps",
            "denied_apps",
            "deniedApps",
            "screenshot_redaction",
            "screenshotRedaction",
            "require_confirmation_destructive",
            "requireConfirmationDestructive",
            "max_actions_per_minute",
            "maxActionsPerMinute",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Enabled,
            AllowedApps,
            DeniedApps,
            ScreenshotRedaction,
            RequireConfirmationDestructive,
            MaxActionsPerMinute,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "enabled" => Ok(GeneratedField::Enabled),
                            "allowedApps" | "allowed_apps" => Ok(GeneratedField::AllowedApps),
                            "deniedApps" | "denied_apps" => Ok(GeneratedField::DeniedApps),
                            "screenshotRedaction" | "screenshot_redaction" => Ok(GeneratedField::ScreenshotRedaction),
                            "requireConfirmationDestructive" | "require_confirmation_destructive" => Ok(GeneratedField::RequireConfirmationDestructive),
                            "maxActionsPerMinute" | "max_actions_per_minute" => Ok(GeneratedField::MaxActionsPerMinute),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = CuaPolicy;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.policy.CuaPolicy")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<CuaPolicy, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut enabled__ = None;
                let mut allowed_apps__ = None;
                let mut denied_apps__ = None;
                let mut screenshot_redaction__ = None;
                let mut require_confirmation_destructive__ = None;
                let mut max_actions_per_minute__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Enabled => {
                            if enabled__.is_some() {
                                return Err(serde::de::Error::duplicate_field("enabled"));
                            }
                            enabled__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AllowedApps => {
                            if allowed_apps__.is_some() {
                                return Err(serde::de::Error::duplicate_field("allowedApps"));
                            }
                            allowed_apps__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DeniedApps => {
                            if denied_apps__.is_some() {
                                return Err(serde::de::Error::duplicate_field("deniedApps"));
                            }
                            denied_apps__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ScreenshotRedaction => {
                            if screenshot_redaction__.is_some() {
                                return Err(serde::de::Error::duplicate_field("screenshotRedaction"));
                            }
                            screenshot_redaction__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RequireConfirmationDestructive => {
                            if require_confirmation_destructive__.is_some() {
                                return Err(serde::de::Error::duplicate_field("requireConfirmationDestructive"));
                            }
                            require_confirmation_destructive__ = Some(map_.next_value()?);
                        }
                        GeneratedField::MaxActionsPerMinute => {
                            if max_actions_per_minute__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxActionsPerMinute"));
                            }
                            max_actions_per_minute__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(CuaPolicy {
                    enabled: enabled__.unwrap_or_default(),
                    allowed_apps: allowed_apps__.unwrap_or_default(),
                    denied_apps: denied_apps__.unwrap_or_default(),
                    screenshot_redaction: screenshot_redaction__.unwrap_or_default(),
                    require_confirmation_destructive: require_confirmation_destructive__.unwrap_or_default(),
                    max_actions_per_minute: max_actions_per_minute__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.policy.CuaPolicy", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for DataClassRule {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.data_class.is_empty() {
            len += 1;
        }
        if self.may_leave_device {
            len += 1;
        }
        if self.requires_redaction {
            len += 1;
        }
        if !self.allowed_destinations.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.policy.DataClassRule", len)?;
        if !self.data_class.is_empty() {
            struct_ser.serialize_field("dataClass", &self.data_class)?;
        }
        if self.may_leave_device {
            struct_ser.serialize_field("mayLeaveDevice", &self.may_leave_device)?;
        }
        if self.requires_redaction {
            struct_ser.serialize_field("requiresRedaction", &self.requires_redaction)?;
        }
        if !self.allowed_destinations.is_empty() {
            struct_ser.serialize_field("allowedDestinations", &self.allowed_destinations)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for DataClassRule {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "data_class",
            "dataClass",
            "may_leave_device",
            "mayLeaveDevice",
            "requires_redaction",
            "requiresRedaction",
            "allowed_destinations",
            "allowedDestinations",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            DataClass,
            MayLeaveDevice,
            RequiresRedaction,
            AllowedDestinations,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "dataClass" | "data_class" => Ok(GeneratedField::DataClass),
                            "mayLeaveDevice" | "may_leave_device" => Ok(GeneratedField::MayLeaveDevice),
                            "requiresRedaction" | "requires_redaction" => Ok(GeneratedField::RequiresRedaction),
                            "allowedDestinations" | "allowed_destinations" => Ok(GeneratedField::AllowedDestinations),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = DataClassRule;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.policy.DataClassRule")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<DataClassRule, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut data_class__ = None;
                let mut may_leave_device__ = None;
                let mut requires_redaction__ = None;
                let mut allowed_destinations__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::DataClass => {
                            if data_class__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dataClass"));
                            }
                            data_class__ = Some(map_.next_value()?);
                        }
                        GeneratedField::MayLeaveDevice => {
                            if may_leave_device__.is_some() {
                                return Err(serde::de::Error::duplicate_field("mayLeaveDevice"));
                            }
                            may_leave_device__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RequiresRedaction => {
                            if requires_redaction__.is_some() {
                                return Err(serde::de::Error::duplicate_field("requiresRedaction"));
                            }
                            requires_redaction__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AllowedDestinations => {
                            if allowed_destinations__.is_some() {
                                return Err(serde::de::Error::duplicate_field("allowedDestinations"));
                            }
                            allowed_destinations__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(DataClassRule {
                    data_class: data_class__.unwrap_or_default(),
                    may_leave_device: may_leave_device__.unwrap_or_default(),
                    requires_redaction: requires_redaction__.unwrap_or_default(),
                    allowed_destinations: allowed_destinations__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.policy.DataClassRule", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for DlpAction {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "DLP_ACTION_UNSPECIFIED",
            Self::Redact => "DLP_ACTION_REDACT",
            Self::Block => "DLP_ACTION_BLOCK",
            Self::LogOnly => "DLP_ACTION_LOG_ONLY",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for DlpAction {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "DLP_ACTION_UNSPECIFIED",
            "DLP_ACTION_REDACT",
            "DLP_ACTION_BLOCK",
            "DLP_ACTION_LOG_ONLY",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = DlpAction;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "expected one of: {:?}", &FIELDS)
            }

            fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Signed(v), &self)
                    })
            }

            fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Unsigned(v), &self)
                    })
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "DLP_ACTION_UNSPECIFIED" => Ok(DlpAction::Unspecified),
                    "DLP_ACTION_REDACT" => Ok(DlpAction::Redact),
                    "DLP_ACTION_BLOCK" => Ok(DlpAction::Block),
                    "DLP_ACTION_LOG_ONLY" => Ok(DlpAction::LogOnly),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for DlpPattern {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.name.is_empty() {
            len += 1;
        }
        if !self.regex.is_empty() {
            len += 1;
        }
        if self.action != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.policy.DlpPattern", len)?;
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.regex.is_empty() {
            struct_ser.serialize_field("regex", &self.regex)?;
        }
        if self.action != 0 {
            let v = DlpAction::try_from(self.action)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.action)))?;
            struct_ser.serialize_field("action", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for DlpPattern {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "regex",
            "action",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
            Regex,
            Action,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "name" => Ok(GeneratedField::Name),
                            "regex" => Ok(GeneratedField::Regex),
                            "action" => Ok(GeneratedField::Action),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = DlpPattern;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.policy.DlpPattern")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<DlpPattern, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut regex__ = None;
                let mut action__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Regex => {
                            if regex__.is_some() {
                                return Err(serde::de::Error::duplicate_field("regex"));
                            }
                            regex__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Action => {
                            if action__.is_some() {
                                return Err(serde::de::Error::duplicate_field("action"));
                            }
                            action__ = Some(map_.next_value::<DlpAction>()? as i32);
                        }
                    }
                }
                Ok(DlpPattern {
                    name: name__.unwrap_or_default(),
                    regex: regex__.unwrap_or_default(),
                    action: action__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.policy.DlpPattern", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for EffectivePolicy {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.endpoint_version.is_empty() {
            len += 1;
        }
        if !self.server_version.is_empty() {
            len += 1;
        }
        if !self.data_rules.is_empty() {
            len += 1;
        }
        if !self.tool_rules.is_empty() {
            len += 1;
        }
        if !self.model_rules.is_empty() {
            len += 1;
        }
        if self.cua.is_some() {
            len += 1;
        }
        if !self.inference_rules.is_empty() {
            len += 1;
        }
        if self.kill_switch {
            len += 1;
        }
        if self.max_retention_hours != 0 {
            len += 1;
        }
        if self.background_quota.is_some() {
            len += 1;
        }
        if self.max_session_duration_hours != 0 {
            len += 1;
        }
        if self.max_concurrent_sessions != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.policy.EffectivePolicy", len)?;
        if !self.endpoint_version.is_empty() {
            struct_ser.serialize_field("endpointVersion", &self.endpoint_version)?;
        }
        if !self.server_version.is_empty() {
            struct_ser.serialize_field("serverVersion", &self.server_version)?;
        }
        if !self.data_rules.is_empty() {
            struct_ser.serialize_field("dataRules", &self.data_rules)?;
        }
        if !self.tool_rules.is_empty() {
            struct_ser.serialize_field("toolRules", &self.tool_rules)?;
        }
        if !self.model_rules.is_empty() {
            struct_ser.serialize_field("modelRules", &self.model_rules)?;
        }
        if let Some(v) = self.cua.as_ref() {
            struct_ser.serialize_field("cua", v)?;
        }
        if !self.inference_rules.is_empty() {
            struct_ser.serialize_field("inferenceRules", &self.inference_rules)?;
        }
        if self.kill_switch {
            struct_ser.serialize_field("killSwitch", &self.kill_switch)?;
        }
        if self.max_retention_hours != 0 {
            struct_ser.serialize_field("maxRetentionHours", &self.max_retention_hours)?;
        }
        if let Some(v) = self.background_quota.as_ref() {
            struct_ser.serialize_field("backgroundQuota", v)?;
        }
        if self.max_session_duration_hours != 0 {
            struct_ser.serialize_field("maxSessionDurationHours", &self.max_session_duration_hours)?;
        }
        if self.max_concurrent_sessions != 0 {
            struct_ser.serialize_field("maxConcurrentSessions", &self.max_concurrent_sessions)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for EffectivePolicy {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "endpoint_version",
            "endpointVersion",
            "server_version",
            "serverVersion",
            "data_rules",
            "dataRules",
            "tool_rules",
            "toolRules",
            "model_rules",
            "modelRules",
            "cua",
            "inference_rules",
            "inferenceRules",
            "kill_switch",
            "killSwitch",
            "max_retention_hours",
            "maxRetentionHours",
            "background_quota",
            "backgroundQuota",
            "max_session_duration_hours",
            "maxSessionDurationHours",
            "max_concurrent_sessions",
            "maxConcurrentSessions",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            EndpointVersion,
            ServerVersion,
            DataRules,
            ToolRules,
            ModelRules,
            Cua,
            InferenceRules,
            KillSwitch,
            MaxRetentionHours,
            BackgroundQuota,
            MaxSessionDurationHours,
            MaxConcurrentSessions,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "endpointVersion" | "endpoint_version" => Ok(GeneratedField::EndpointVersion),
                            "serverVersion" | "server_version" => Ok(GeneratedField::ServerVersion),
                            "dataRules" | "data_rules" => Ok(GeneratedField::DataRules),
                            "toolRules" | "tool_rules" => Ok(GeneratedField::ToolRules),
                            "modelRules" | "model_rules" => Ok(GeneratedField::ModelRules),
                            "cua" => Ok(GeneratedField::Cua),
                            "inferenceRules" | "inference_rules" => Ok(GeneratedField::InferenceRules),
                            "killSwitch" | "kill_switch" => Ok(GeneratedField::KillSwitch),
                            "maxRetentionHours" | "max_retention_hours" => Ok(GeneratedField::MaxRetentionHours),
                            "backgroundQuota" | "background_quota" => Ok(GeneratedField::BackgroundQuota),
                            "maxSessionDurationHours" | "max_session_duration_hours" => Ok(GeneratedField::MaxSessionDurationHours),
                            "maxConcurrentSessions" | "max_concurrent_sessions" => Ok(GeneratedField::MaxConcurrentSessions),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = EffectivePolicy;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.policy.EffectivePolicy")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<EffectivePolicy, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut endpoint_version__ = None;
                let mut server_version__ = None;
                let mut data_rules__ = None;
                let mut tool_rules__ = None;
                let mut model_rules__ = None;
                let mut cua__ = None;
                let mut inference_rules__ = None;
                let mut kill_switch__ = None;
                let mut max_retention_hours__ = None;
                let mut background_quota__ = None;
                let mut max_session_duration_hours__ = None;
                let mut max_concurrent_sessions__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::EndpointVersion => {
                            if endpoint_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("endpointVersion"));
                            }
                            endpoint_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ServerVersion => {
                            if server_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("serverVersion"));
                            }
                            server_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DataRules => {
                            if data_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dataRules"));
                            }
                            data_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ToolRules => {
                            if tool_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("toolRules"));
                            }
                            tool_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ModelRules => {
                            if model_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("modelRules"));
                            }
                            model_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Cua => {
                            if cua__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cua"));
                            }
                            cua__ = map_.next_value()?;
                        }
                        GeneratedField::InferenceRules => {
                            if inference_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("inferenceRules"));
                            }
                            inference_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::KillSwitch => {
                            if kill_switch__.is_some() {
                                return Err(serde::de::Error::duplicate_field("killSwitch"));
                            }
                            kill_switch__ = Some(map_.next_value()?);
                        }
                        GeneratedField::MaxRetentionHours => {
                            if max_retention_hours__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxRetentionHours"));
                            }
                            max_retention_hours__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::BackgroundQuota => {
                            if background_quota__.is_some() {
                                return Err(serde::de::Error::duplicate_field("backgroundQuota"));
                            }
                            background_quota__ = map_.next_value()?;
                        }
                        GeneratedField::MaxSessionDurationHours => {
                            if max_session_duration_hours__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxSessionDurationHours"));
                            }
                            max_session_duration_hours__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::MaxConcurrentSessions => {
                            if max_concurrent_sessions__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxConcurrentSessions"));
                            }
                            max_concurrent_sessions__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(EffectivePolicy {
                    endpoint_version: endpoint_version__.unwrap_or_default(),
                    server_version: server_version__.unwrap_or_default(),
                    data_rules: data_rules__.unwrap_or_default(),
                    tool_rules: tool_rules__.unwrap_or_default(),
                    model_rules: model_rules__.unwrap_or_default(),
                    cua: cua__,
                    inference_rules: inference_rules__.unwrap_or_default(),
                    kill_switch: kill_switch__.unwrap_or_default(),
                    max_retention_hours: max_retention_hours__.unwrap_or_default(),
                    background_quota: background_quota__,
                    max_session_duration_hours: max_session_duration_hours__.unwrap_or_default(),
                    max_concurrent_sessions: max_concurrent_sessions__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.policy.EffectivePolicy", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for EndpointPolicy {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.policy_id.is_empty() {
            len += 1;
        }
        if !self.version.is_empty() {
            len += 1;
        }
        if !self.org_id.is_empty() {
            len += 1;
        }
        if !self.data_rules.is_empty() {
            len += 1;
        }
        if !self.tool_rules.is_empty() {
            len += 1;
        }
        if !self.model_rules.is_empty() {
            len += 1;
        }
        if self.cua.is_some() {
            len += 1;
        }
        if self.kill_switch {
            len += 1;
        }
        if self.max_retention_hours != 0 {
            len += 1;
        }
        if !self.dlp_patterns.is_empty() {
            len += 1;
        }
        if self.safety.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.policy.EndpointPolicy", len)?;
        if !self.policy_id.is_empty() {
            struct_ser.serialize_field("policyId", &self.policy_id)?;
        }
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        if !self.org_id.is_empty() {
            struct_ser.serialize_field("orgId", &self.org_id)?;
        }
        if !self.data_rules.is_empty() {
            struct_ser.serialize_field("dataRules", &self.data_rules)?;
        }
        if !self.tool_rules.is_empty() {
            struct_ser.serialize_field("toolRules", &self.tool_rules)?;
        }
        if !self.model_rules.is_empty() {
            struct_ser.serialize_field("modelRules", &self.model_rules)?;
        }
        if let Some(v) = self.cua.as_ref() {
            struct_ser.serialize_field("cua", v)?;
        }
        if self.kill_switch {
            struct_ser.serialize_field("killSwitch", &self.kill_switch)?;
        }
        if self.max_retention_hours != 0 {
            struct_ser.serialize_field("maxRetentionHours", &self.max_retention_hours)?;
        }
        if !self.dlp_patterns.is_empty() {
            struct_ser.serialize_field("dlpPatterns", &self.dlp_patterns)?;
        }
        if let Some(v) = self.safety.as_ref() {
            struct_ser.serialize_field("safety", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for EndpointPolicy {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "policy_id",
            "policyId",
            "version",
            "org_id",
            "orgId",
            "data_rules",
            "dataRules",
            "tool_rules",
            "toolRules",
            "model_rules",
            "modelRules",
            "cua",
            "kill_switch",
            "killSwitch",
            "max_retention_hours",
            "maxRetentionHours",
            "dlp_patterns",
            "dlpPatterns",
            "safety",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            PolicyId,
            Version,
            OrgId,
            DataRules,
            ToolRules,
            ModelRules,
            Cua,
            KillSwitch,
            MaxRetentionHours,
            DlpPatterns,
            Safety,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "policyId" | "policy_id" => Ok(GeneratedField::PolicyId),
                            "version" => Ok(GeneratedField::Version),
                            "orgId" | "org_id" => Ok(GeneratedField::OrgId),
                            "dataRules" | "data_rules" => Ok(GeneratedField::DataRules),
                            "toolRules" | "tool_rules" => Ok(GeneratedField::ToolRules),
                            "modelRules" | "model_rules" => Ok(GeneratedField::ModelRules),
                            "cua" => Ok(GeneratedField::Cua),
                            "killSwitch" | "kill_switch" => Ok(GeneratedField::KillSwitch),
                            "maxRetentionHours" | "max_retention_hours" => Ok(GeneratedField::MaxRetentionHours),
                            "dlpPatterns" | "dlp_patterns" => Ok(GeneratedField::DlpPatterns),
                            "safety" => Ok(GeneratedField::Safety),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = EndpointPolicy;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.policy.EndpointPolicy")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<EndpointPolicy, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut policy_id__ = None;
                let mut version__ = None;
                let mut org_id__ = None;
                let mut data_rules__ = None;
                let mut tool_rules__ = None;
                let mut model_rules__ = None;
                let mut cua__ = None;
                let mut kill_switch__ = None;
                let mut max_retention_hours__ = None;
                let mut dlp_patterns__ = None;
                let mut safety__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::PolicyId => {
                            if policy_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("policyId"));
                            }
                            policy_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::OrgId => {
                            if org_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("orgId"));
                            }
                            org_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DataRules => {
                            if data_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dataRules"));
                            }
                            data_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ToolRules => {
                            if tool_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("toolRules"));
                            }
                            tool_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ModelRules => {
                            if model_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("modelRules"));
                            }
                            model_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Cua => {
                            if cua__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cua"));
                            }
                            cua__ = map_.next_value()?;
                        }
                        GeneratedField::KillSwitch => {
                            if kill_switch__.is_some() {
                                return Err(serde::de::Error::duplicate_field("killSwitch"));
                            }
                            kill_switch__ = Some(map_.next_value()?);
                        }
                        GeneratedField::MaxRetentionHours => {
                            if max_retention_hours__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxRetentionHours"));
                            }
                            max_retention_hours__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DlpPatterns => {
                            if dlp_patterns__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dlpPatterns"));
                            }
                            dlp_patterns__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Safety => {
                            if safety__.is_some() {
                                return Err(serde::de::Error::duplicate_field("safety"));
                            }
                            safety__ = map_.next_value()?;
                        }
                    }
                }
                Ok(EndpointPolicy {
                    policy_id: policy_id__.unwrap_or_default(),
                    version: version__.unwrap_or_default(),
                    org_id: org_id__.unwrap_or_default(),
                    data_rules: data_rules__.unwrap_or_default(),
                    tool_rules: tool_rules__.unwrap_or_default(),
                    model_rules: model_rules__.unwrap_or_default(),
                    cua: cua__,
                    kill_switch: kill_switch__.unwrap_or_default(),
                    max_retention_hours: max_retention_hours__.unwrap_or_default(),
                    dlp_patterns: dlp_patterns__.unwrap_or_default(),
                    safety: safety__,
                })
            }
        }
        deserializer.deserialize_struct("fabric.policy.EndpointPolicy", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for FailMode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "FAIL_MODE_UNSPECIFIED",
            Self::Closed => "FAIL_MODE_CLOSED",
            Self::Open => "FAIL_MODE_OPEN",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for FailMode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "FAIL_MODE_UNSPECIFIED",
            "FAIL_MODE_CLOSED",
            "FAIL_MODE_OPEN",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = FailMode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "expected one of: {:?}", &FIELDS)
            }

            fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Signed(v), &self)
                    })
            }

            fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Unsigned(v), &self)
                    })
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "FAIL_MODE_UNSPECIFIED" => Ok(FailMode::Unspecified),
                    "FAIL_MODE_CLOSED" => Ok(FailMode::Closed),
                    "FAIL_MODE_OPEN" => Ok(FailMode::Open),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for InferenceRule {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.provider.is_empty() {
            len += 1;
        }
        if !self.allowed_models.is_empty() {
            len += 1;
        }
        if !self.allowed_regions.is_empty() {
            len += 1;
        }
        if self.max_tokens_per_request != 0 {
            len += 1;
        }
        if self.daily_token_budget != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.policy.InferenceRule", len)?;
        if !self.provider.is_empty() {
            struct_ser.serialize_field("provider", &self.provider)?;
        }
        if !self.allowed_models.is_empty() {
            struct_ser.serialize_field("allowedModels", &self.allowed_models)?;
        }
        if !self.allowed_regions.is_empty() {
            struct_ser.serialize_field("allowedRegions", &self.allowed_regions)?;
        }
        if self.max_tokens_per_request != 0 {
            struct_ser.serialize_field("maxTokensPerRequest", &self.max_tokens_per_request)?;
        }
        if self.daily_token_budget != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("dailyTokenBudget", ToString::to_string(&self.daily_token_budget).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for InferenceRule {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "provider",
            "allowed_models",
            "allowedModels",
            "allowed_regions",
            "allowedRegions",
            "max_tokens_per_request",
            "maxTokensPerRequest",
            "daily_token_budget",
            "dailyTokenBudget",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Provider,
            AllowedModels,
            AllowedRegions,
            MaxTokensPerRequest,
            DailyTokenBudget,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "provider" => Ok(GeneratedField::Provider),
                            "allowedModels" | "allowed_models" => Ok(GeneratedField::AllowedModels),
                            "allowedRegions" | "allowed_regions" => Ok(GeneratedField::AllowedRegions),
                            "maxTokensPerRequest" | "max_tokens_per_request" => Ok(GeneratedField::MaxTokensPerRequest),
                            "dailyTokenBudget" | "daily_token_budget" => Ok(GeneratedField::DailyTokenBudget),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = InferenceRule;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.policy.InferenceRule")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<InferenceRule, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut provider__ = None;
                let mut allowed_models__ = None;
                let mut allowed_regions__ = None;
                let mut max_tokens_per_request__ = None;
                let mut daily_token_budget__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Provider => {
                            if provider__.is_some() {
                                return Err(serde::de::Error::duplicate_field("provider"));
                            }
                            provider__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AllowedModels => {
                            if allowed_models__.is_some() {
                                return Err(serde::de::Error::duplicate_field("allowedModels"));
                            }
                            allowed_models__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AllowedRegions => {
                            if allowed_regions__.is_some() {
                                return Err(serde::de::Error::duplicate_field("allowedRegions"));
                            }
                            allowed_regions__ = Some(map_.next_value()?);
                        }
                        GeneratedField::MaxTokensPerRequest => {
                            if max_tokens_per_request__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxTokensPerRequest"));
                            }
                            max_tokens_per_request__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DailyTokenBudget => {
                            if daily_token_budget__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dailyTokenBudget"));
                            }
                            daily_token_budget__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(InferenceRule {
                    provider: provider__.unwrap_or_default(),
                    allowed_models: allowed_models__.unwrap_or_default(),
                    allowed_regions: allowed_regions__.unwrap_or_default(),
                    max_tokens_per_request: max_tokens_per_request__.unwrap_or_default(),
                    daily_token_budget: daily_token_budget__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.policy.InferenceRule", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ModelRule {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.model_pattern.is_empty() {
            len += 1;
        }
        if self.allowed_local {
            len += 1;
        }
        if self.allowed_server {
            len += 1;
        }
        if self.max_context_tokens != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.policy.ModelRule", len)?;
        if !self.model_pattern.is_empty() {
            struct_ser.serialize_field("modelPattern", &self.model_pattern)?;
        }
        if self.allowed_local {
            struct_ser.serialize_field("allowedLocal", &self.allowed_local)?;
        }
        if self.allowed_server {
            struct_ser.serialize_field("allowedServer", &self.allowed_server)?;
        }
        if self.max_context_tokens != 0 {
            struct_ser.serialize_field("maxContextTokens", &self.max_context_tokens)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ModelRule {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "model_pattern",
            "modelPattern",
            "allowed_local",
            "allowedLocal",
            "allowed_server",
            "allowedServer",
            "max_context_tokens",
            "maxContextTokens",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ModelPattern,
            AllowedLocal,
            AllowedServer,
            MaxContextTokens,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "modelPattern" | "model_pattern" => Ok(GeneratedField::ModelPattern),
                            "allowedLocal" | "allowed_local" => Ok(GeneratedField::AllowedLocal),
                            "allowedServer" | "allowed_server" => Ok(GeneratedField::AllowedServer),
                            "maxContextTokens" | "max_context_tokens" => Ok(GeneratedField::MaxContextTokens),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ModelRule;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.policy.ModelRule")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ModelRule, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut model_pattern__ = None;
                let mut allowed_local__ = None;
                let mut allowed_server__ = None;
                let mut max_context_tokens__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ModelPattern => {
                            if model_pattern__.is_some() {
                                return Err(serde::de::Error::duplicate_field("modelPattern"));
                            }
                            model_pattern__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AllowedLocal => {
                            if allowed_local__.is_some() {
                                return Err(serde::de::Error::duplicate_field("allowedLocal"));
                            }
                            allowed_local__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AllowedServer => {
                            if allowed_server__.is_some() {
                                return Err(serde::de::Error::duplicate_field("allowedServer"));
                            }
                            allowed_server__ = Some(map_.next_value()?);
                        }
                        GeneratedField::MaxContextTokens => {
                            if max_context_tokens__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxContextTokens"));
                            }
                            max_context_tokens__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(ModelRule {
                    model_pattern: model_pattern__.unwrap_or_default(),
                    allowed_local: allowed_local__.unwrap_or_default(),
                    allowed_server: allowed_server__.unwrap_or_default(),
                    max_context_tokens: max_context_tokens__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.policy.ModelRule", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SafetyAction {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "SAFETY_ACTION_UNSPECIFIED",
            Self::Allow => "SAFETY_ACTION_ALLOW",
            Self::Warn => "SAFETY_ACTION_WARN",
            Self::ForceEndpoint => "SAFETY_ACTION_FORCE_ENDPOINT",
            Self::Block => "SAFETY_ACTION_BLOCK",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for SafetyAction {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "SAFETY_ACTION_UNSPECIFIED",
            "SAFETY_ACTION_ALLOW",
            "SAFETY_ACTION_WARN",
            "SAFETY_ACTION_FORCE_ENDPOINT",
            "SAFETY_ACTION_BLOCK",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SafetyAction;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "expected one of: {:?}", &FIELDS)
            }

            fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Signed(v), &self)
                    })
            }

            fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Unsigned(v), &self)
                    })
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "SAFETY_ACTION_UNSPECIFIED" => Ok(SafetyAction::Unspecified),
                    "SAFETY_ACTION_ALLOW" => Ok(SafetyAction::Allow),
                    "SAFETY_ACTION_WARN" => Ok(SafetyAction::Warn),
                    "SAFETY_ACTION_FORCE_ENDPOINT" => Ok(SafetyAction::ForceEndpoint),
                    "SAFETY_ACTION_BLOCK" => Ok(SafetyAction::Block),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for SafetyConfig {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.endpoint_url.is_empty() {
            len += 1;
        }
        if !self.model.is_empty() {
            len += 1;
        }
        if !self.parser.is_empty() {
            len += 1;
        }
        if self.timeout_ms != 0 {
            len += 1;
        }
        if self.fail_mode != 0 {
            len += 1;
        }
        if !self.rules.is_empty() {
            len += 1;
        }
        if self.default_action != 0 {
            len += 1;
        }
        if !self.api_key.is_empty() {
            len += 1;
        }
        if !self.extra_body_json.is_empty() {
            len += 1;
        }
        if !self.system_prompt.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.policy.SafetyConfig", len)?;
        if !self.endpoint_url.is_empty() {
            struct_ser.serialize_field("endpointUrl", &self.endpoint_url)?;
        }
        if !self.model.is_empty() {
            struct_ser.serialize_field("model", &self.model)?;
        }
        if !self.parser.is_empty() {
            struct_ser.serialize_field("parser", &self.parser)?;
        }
        if self.timeout_ms != 0 {
            struct_ser.serialize_field("timeoutMs", &self.timeout_ms)?;
        }
        if self.fail_mode != 0 {
            let v = FailMode::try_from(self.fail_mode)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.fail_mode)))?;
            struct_ser.serialize_field("failMode", &v)?;
        }
        if !self.rules.is_empty() {
            struct_ser.serialize_field("rules", &self.rules)?;
        }
        if self.default_action != 0 {
            let v = SafetyAction::try_from(self.default_action)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.default_action)))?;
            struct_ser.serialize_field("defaultAction", &v)?;
        }
        if !self.api_key.is_empty() {
            struct_ser.serialize_field("apiKey", &self.api_key)?;
        }
        if !self.extra_body_json.is_empty() {
            struct_ser.serialize_field("extraBodyJson", &self.extra_body_json)?;
        }
        if !self.system_prompt.is_empty() {
            struct_ser.serialize_field("systemPrompt", &self.system_prompt)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SafetyConfig {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "endpoint_url",
            "endpointUrl",
            "model",
            "parser",
            "timeout_ms",
            "timeoutMs",
            "fail_mode",
            "failMode",
            "rules",
            "default_action",
            "defaultAction",
            "api_key",
            "apiKey",
            "extra_body_json",
            "extraBodyJson",
            "system_prompt",
            "systemPrompt",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            EndpointUrl,
            Model,
            Parser,
            TimeoutMs,
            FailMode,
            Rules,
            DefaultAction,
            ApiKey,
            ExtraBodyJson,
            SystemPrompt,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "endpointUrl" | "endpoint_url" => Ok(GeneratedField::EndpointUrl),
                            "model" => Ok(GeneratedField::Model),
                            "parser" => Ok(GeneratedField::Parser),
                            "timeoutMs" | "timeout_ms" => Ok(GeneratedField::TimeoutMs),
                            "failMode" | "fail_mode" => Ok(GeneratedField::FailMode),
                            "rules" => Ok(GeneratedField::Rules),
                            "defaultAction" | "default_action" => Ok(GeneratedField::DefaultAction),
                            "apiKey" | "api_key" => Ok(GeneratedField::ApiKey),
                            "extraBodyJson" | "extra_body_json" => Ok(GeneratedField::ExtraBodyJson),
                            "systemPrompt" | "system_prompt" => Ok(GeneratedField::SystemPrompt),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SafetyConfig;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.policy.SafetyConfig")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SafetyConfig, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut endpoint_url__ = None;
                let mut model__ = None;
                let mut parser__ = None;
                let mut timeout_ms__ = None;
                let mut fail_mode__ = None;
                let mut rules__ = None;
                let mut default_action__ = None;
                let mut api_key__ = None;
                let mut extra_body_json__ = None;
                let mut system_prompt__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::EndpointUrl => {
                            if endpoint_url__.is_some() {
                                return Err(serde::de::Error::duplicate_field("endpointUrl"));
                            }
                            endpoint_url__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Model => {
                            if model__.is_some() {
                                return Err(serde::de::Error::duplicate_field("model"));
                            }
                            model__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Parser => {
                            if parser__.is_some() {
                                return Err(serde::de::Error::duplicate_field("parser"));
                            }
                            parser__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TimeoutMs => {
                            if timeout_ms__.is_some() {
                                return Err(serde::de::Error::duplicate_field("timeoutMs"));
                            }
                            timeout_ms__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::FailMode => {
                            if fail_mode__.is_some() {
                                return Err(serde::de::Error::duplicate_field("failMode"));
                            }
                            fail_mode__ = Some(map_.next_value::<FailMode>()? as i32);
                        }
                        GeneratedField::Rules => {
                            if rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("rules"));
                            }
                            rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DefaultAction => {
                            if default_action__.is_some() {
                                return Err(serde::de::Error::duplicate_field("defaultAction"));
                            }
                            default_action__ = Some(map_.next_value::<SafetyAction>()? as i32);
                        }
                        GeneratedField::ApiKey => {
                            if api_key__.is_some() {
                                return Err(serde::de::Error::duplicate_field("apiKey"));
                            }
                            api_key__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ExtraBodyJson => {
                            if extra_body_json__.is_some() {
                                return Err(serde::de::Error::duplicate_field("extraBodyJson"));
                            }
                            extra_body_json__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SystemPrompt => {
                            if system_prompt__.is_some() {
                                return Err(serde::de::Error::duplicate_field("systemPrompt"));
                            }
                            system_prompt__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(SafetyConfig {
                    endpoint_url: endpoint_url__.unwrap_or_default(),
                    model: model__.unwrap_or_default(),
                    parser: parser__.unwrap_or_default(),
                    timeout_ms: timeout_ms__.unwrap_or_default(),
                    fail_mode: fail_mode__.unwrap_or_default(),
                    rules: rules__.unwrap_or_default(),
                    default_action: default_action__.unwrap_or_default(),
                    api_key: api_key__.unwrap_or_default(),
                    extra_body_json: extra_body_json__.unwrap_or_default(),
                    system_prompt: system_prompt__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.policy.SafetyConfig", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SafetyPolicyRule {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.category.is_empty() {
            len += 1;
        }
        if self.action != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.policy.SafetyPolicyRule", len)?;
        if !self.category.is_empty() {
            struct_ser.serialize_field("category", &self.category)?;
        }
        if self.action != 0 {
            let v = SafetyAction::try_from(self.action)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.action)))?;
            struct_ser.serialize_field("action", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SafetyPolicyRule {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "category",
            "action",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Category,
            Action,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "category" => Ok(GeneratedField::Category),
                            "action" => Ok(GeneratedField::Action),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SafetyPolicyRule;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.policy.SafetyPolicyRule")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SafetyPolicyRule, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut category__ = None;
                let mut action__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Category => {
                            if category__.is_some() {
                                return Err(serde::de::Error::duplicate_field("category"));
                            }
                            category__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Action => {
                            if action__.is_some() {
                                return Err(serde::de::Error::duplicate_field("action"));
                            }
                            action__ = Some(map_.next_value::<SafetyAction>()? as i32);
                        }
                    }
                }
                Ok(SafetyPolicyRule {
                    category: category__.unwrap_or_default(),
                    action: action__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.policy.SafetyPolicyRule", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ServerPolicy {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.policy_id.is_empty() {
            len += 1;
        }
        if !self.version.is_empty() {
            len += 1;
        }
        if !self.org_id.is_empty() {
            len += 1;
        }
        if !self.inference_rules.is_empty() {
            len += 1;
        }
        if self.background_quota.is_some() {
            len += 1;
        }
        if !self.tool_restrictions.is_empty() {
            len += 1;
        }
        if self.max_session_duration_hours != 0 {
            len += 1;
        }
        if self.max_concurrent_sessions != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.policy.ServerPolicy", len)?;
        if !self.policy_id.is_empty() {
            struct_ser.serialize_field("policyId", &self.policy_id)?;
        }
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        if !self.org_id.is_empty() {
            struct_ser.serialize_field("orgId", &self.org_id)?;
        }
        if !self.inference_rules.is_empty() {
            struct_ser.serialize_field("inferenceRules", &self.inference_rules)?;
        }
        if let Some(v) = self.background_quota.as_ref() {
            struct_ser.serialize_field("backgroundQuota", v)?;
        }
        if !self.tool_restrictions.is_empty() {
            struct_ser.serialize_field("toolRestrictions", &self.tool_restrictions)?;
        }
        if self.max_session_duration_hours != 0 {
            struct_ser.serialize_field("maxSessionDurationHours", &self.max_session_duration_hours)?;
        }
        if self.max_concurrent_sessions != 0 {
            struct_ser.serialize_field("maxConcurrentSessions", &self.max_concurrent_sessions)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ServerPolicy {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "policy_id",
            "policyId",
            "version",
            "org_id",
            "orgId",
            "inference_rules",
            "inferenceRules",
            "background_quota",
            "backgroundQuota",
            "tool_restrictions",
            "toolRestrictions",
            "max_session_duration_hours",
            "maxSessionDurationHours",
            "max_concurrent_sessions",
            "maxConcurrentSessions",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            PolicyId,
            Version,
            OrgId,
            InferenceRules,
            BackgroundQuota,
            ToolRestrictions,
            MaxSessionDurationHours,
            MaxConcurrentSessions,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "policyId" | "policy_id" => Ok(GeneratedField::PolicyId),
                            "version" => Ok(GeneratedField::Version),
                            "orgId" | "org_id" => Ok(GeneratedField::OrgId),
                            "inferenceRules" | "inference_rules" => Ok(GeneratedField::InferenceRules),
                            "backgroundQuota" | "background_quota" => Ok(GeneratedField::BackgroundQuota),
                            "toolRestrictions" | "tool_restrictions" => Ok(GeneratedField::ToolRestrictions),
                            "maxSessionDurationHours" | "max_session_duration_hours" => Ok(GeneratedField::MaxSessionDurationHours),
                            "maxConcurrentSessions" | "max_concurrent_sessions" => Ok(GeneratedField::MaxConcurrentSessions),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ServerPolicy;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.policy.ServerPolicy")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ServerPolicy, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut policy_id__ = None;
                let mut version__ = None;
                let mut org_id__ = None;
                let mut inference_rules__ = None;
                let mut background_quota__ = None;
                let mut tool_restrictions__ = None;
                let mut max_session_duration_hours__ = None;
                let mut max_concurrent_sessions__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::PolicyId => {
                            if policy_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("policyId"));
                            }
                            policy_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::OrgId => {
                            if org_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("orgId"));
                            }
                            org_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::InferenceRules => {
                            if inference_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("inferenceRules"));
                            }
                            inference_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::BackgroundQuota => {
                            if background_quota__.is_some() {
                                return Err(serde::de::Error::duplicate_field("backgroundQuota"));
                            }
                            background_quota__ = map_.next_value()?;
                        }
                        GeneratedField::ToolRestrictions => {
                            if tool_restrictions__.is_some() {
                                return Err(serde::de::Error::duplicate_field("toolRestrictions"));
                            }
                            tool_restrictions__ = Some(map_.next_value()?);
                        }
                        GeneratedField::MaxSessionDurationHours => {
                            if max_session_duration_hours__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxSessionDurationHours"));
                            }
                            max_session_duration_hours__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::MaxConcurrentSessions => {
                            if max_concurrent_sessions__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxConcurrentSessions"));
                            }
                            max_concurrent_sessions__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(ServerPolicy {
                    policy_id: policy_id__.unwrap_or_default(),
                    version: version__.unwrap_or_default(),
                    org_id: org_id__.unwrap_or_default(),
                    inference_rules: inference_rules__.unwrap_or_default(),
                    background_quota: background_quota__,
                    tool_restrictions: tool_restrictions__.unwrap_or_default(),
                    max_session_duration_hours: max_session_duration_hours__.unwrap_or_default(),
                    max_concurrent_sessions: max_concurrent_sessions__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.policy.ServerPolicy", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ToolAction {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "TOOL_ACTION_UNSPECIFIED",
            Self::Allow => "TOOL_ACTION_ALLOW",
            Self::Deny => "TOOL_ACTION_DENY",
            Self::RequireApproval => "TOOL_ACTION_REQUIRE_APPROVAL",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ToolAction {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "TOOL_ACTION_UNSPECIFIED",
            "TOOL_ACTION_ALLOW",
            "TOOL_ACTION_DENY",
            "TOOL_ACTION_REQUIRE_APPROVAL",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ToolAction;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "expected one of: {:?}", &FIELDS)
            }

            fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Signed(v), &self)
                    })
            }

            fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Unsigned(v), &self)
                    })
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "TOOL_ACTION_UNSPECIFIED" => Ok(ToolAction::Unspecified),
                    "TOOL_ACTION_ALLOW" => Ok(ToolAction::Allow),
                    "TOOL_ACTION_DENY" => Ok(ToolAction::Deny),
                    "TOOL_ACTION_REQUIRE_APPROVAL" => Ok(ToolAction::RequireApproval),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ToolRule {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.tool_pattern.is_empty() {
            len += 1;
        }
        if self.action != 0 {
            len += 1;
        }
        if !self.condition.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.policy.ToolRule", len)?;
        if !self.tool_pattern.is_empty() {
            struct_ser.serialize_field("toolPattern", &self.tool_pattern)?;
        }
        if self.action != 0 {
            let v = ToolAction::try_from(self.action)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.action)))?;
            struct_ser.serialize_field("action", &v)?;
        }
        if !self.condition.is_empty() {
            struct_ser.serialize_field("condition", &self.condition)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ToolRule {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "tool_pattern",
            "toolPattern",
            "action",
            "condition",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ToolPattern,
            Action,
            Condition,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "toolPattern" | "tool_pattern" => Ok(GeneratedField::ToolPattern),
                            "action" => Ok(GeneratedField::Action),
                            "condition" => Ok(GeneratedField::Condition),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ToolRule;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.policy.ToolRule")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ToolRule, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut tool_pattern__ = None;
                let mut action__ = None;
                let mut condition__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ToolPattern => {
                            if tool_pattern__.is_some() {
                                return Err(serde::de::Error::duplicate_field("toolPattern"));
                            }
                            tool_pattern__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Action => {
                            if action__.is_some() {
                                return Err(serde::de::Error::duplicate_field("action"));
                            }
                            action__ = Some(map_.next_value::<ToolAction>()? as i32);
                        }
                        GeneratedField::Condition => {
                            if condition__.is_some() {
                                return Err(serde::de::Error::duplicate_field("condition"));
                            }
                            condition__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(ToolRule {
                    tool_pattern: tool_pattern__.unwrap_or_default(),
                    action: action__.unwrap_or_default(),
                    condition: condition__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.policy.ToolRule", FIELDS, GeneratedVisitor)
    }
}
