// @generated
impl serde::Serialize for ContextEntry {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.entry_id.is_empty() {
            len += 1;
        }
        if !self.session_id.is_empty() {
            len += 1;
        }
        if self.seq != 0 {
            len += 1;
        }
        if self.kind != 0 {
            len += 1;
        }
        if !self.payload.is_empty() {
            len += 1;
        }
        if !self.lease_holder.is_empty() {
            len += 1;
        }
        if !self.policy_version.is_empty() {
            len += 1;
        }
        if self.locus != 0 {
            len += 1;
        }
        if self.created_at.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.context.ContextEntry", len)?;
        if !self.entry_id.is_empty() {
            struct_ser.serialize_field("entryId", &self.entry_id)?;
        }
        if !self.session_id.is_empty() {
            struct_ser.serialize_field("sessionId", &self.session_id)?;
        }
        if self.seq != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("seq", ToString::to_string(&self.seq).as_str())?;
        }
        if self.kind != 0 {
            let v = EntryKind::try_from(self.kind)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.kind)))?;
            struct_ser.serialize_field("kind", &v)?;
        }
        if !self.payload.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("payload", pbjson::private::base64::encode(&self.payload).as_str())?;
        }
        if !self.lease_holder.is_empty() {
            struct_ser.serialize_field("leaseHolder", &self.lease_holder)?;
        }
        if !self.policy_version.is_empty() {
            struct_ser.serialize_field("policyVersion", &self.policy_version)?;
        }
        if self.locus != 0 {
            let v = Locus::try_from(self.locus)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.locus)))?;
            struct_ser.serialize_field("locus", &v)?;
        }
        if let Some(v) = self.created_at.as_ref() {
            struct_ser.serialize_field("createdAt", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ContextEntry {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "entry_id",
            "entryId",
            "session_id",
            "sessionId",
            "seq",
            "kind",
            "payload",
            "lease_holder",
            "leaseHolder",
            "policy_version",
            "policyVersion",
            "locus",
            "created_at",
            "createdAt",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            EntryId,
            SessionId,
            Seq,
            Kind,
            Payload,
            LeaseHolder,
            PolicyVersion,
            Locus,
            CreatedAt,
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
                            "entryId" | "entry_id" => Ok(GeneratedField::EntryId),
                            "sessionId" | "session_id" => Ok(GeneratedField::SessionId),
                            "seq" => Ok(GeneratedField::Seq),
                            "kind" => Ok(GeneratedField::Kind),
                            "payload" => Ok(GeneratedField::Payload),
                            "leaseHolder" | "lease_holder" => Ok(GeneratedField::LeaseHolder),
                            "policyVersion" | "policy_version" => Ok(GeneratedField::PolicyVersion),
                            "locus" => Ok(GeneratedField::Locus),
                            "createdAt" | "created_at" => Ok(GeneratedField::CreatedAt),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ContextEntry;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.context.ContextEntry")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ContextEntry, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut entry_id__ = None;
                let mut session_id__ = None;
                let mut seq__ = None;
                let mut kind__ = None;
                let mut payload__ = None;
                let mut lease_holder__ = None;
                let mut policy_version__ = None;
                let mut locus__ = None;
                let mut created_at__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::EntryId => {
                            if entry_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("entryId"));
                            }
                            entry_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SessionId => {
                            if session_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sessionId"));
                            }
                            session_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Seq => {
                            if seq__.is_some() {
                                return Err(serde::de::Error::duplicate_field("seq"));
                            }
                            seq__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Kind => {
                            if kind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("kind"));
                            }
                            kind__ = Some(map_.next_value::<EntryKind>()? as i32);
                        }
                        GeneratedField::Payload => {
                            if payload__.is_some() {
                                return Err(serde::de::Error::duplicate_field("payload"));
                            }
                            payload__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::LeaseHolder => {
                            if lease_holder__.is_some() {
                                return Err(serde::de::Error::duplicate_field("leaseHolder"));
                            }
                            lease_holder__ = Some(map_.next_value()?);
                        }
                        GeneratedField::PolicyVersion => {
                            if policy_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("policyVersion"));
                            }
                            policy_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Locus => {
                            if locus__.is_some() {
                                return Err(serde::de::Error::duplicate_field("locus"));
                            }
                            locus__ = Some(map_.next_value::<Locus>()? as i32);
                        }
                        GeneratedField::CreatedAt => {
                            if created_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("createdAt"));
                            }
                            created_at__ = map_.next_value()?;
                        }
                    }
                }
                Ok(ContextEntry {
                    entry_id: entry_id__.unwrap_or_default(),
                    session_id: session_id__.unwrap_or_default(),
                    seq: seq__.unwrap_or_default(),
                    kind: kind__.unwrap_or_default(),
                    payload: payload__.unwrap_or_default(),
                    lease_holder: lease_holder__.unwrap_or_default(),
                    policy_version: policy_version__.unwrap_or_default(),
                    locus: locus__.unwrap_or_default(),
                    created_at: created_at__,
                })
            }
        }
        deserializer.deserialize_struct("fabric.context.ContextEntry", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for EntryKind {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "ENTRY_KIND_UNSPECIFIED",
            Self::UserMessage => "ENTRY_KIND_USER_MESSAGE",
            Self::AssistantMessage => "ENTRY_KIND_ASSISTANT_MESSAGE",
            Self::ToolCall => "ENTRY_KIND_TOOL_CALL",
            Self::ToolResult => "ENTRY_KIND_TOOL_RESULT",
            Self::SystemEvent => "ENTRY_KIND_SYSTEM_EVENT",
            Self::GoalUpdate => "ENTRY_KIND_GOAL_UPDATE",
            Self::PlanStep => "ENTRY_KIND_PLAN_STEP",
            Self::HandoffMarker => "ENTRY_KIND_HANDOFF_MARKER",
            Self::DeferredIntent => "ENTRY_KIND_DEFERRED_INTENT",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for EntryKind {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ENTRY_KIND_UNSPECIFIED",
            "ENTRY_KIND_USER_MESSAGE",
            "ENTRY_KIND_ASSISTANT_MESSAGE",
            "ENTRY_KIND_TOOL_CALL",
            "ENTRY_KIND_TOOL_RESULT",
            "ENTRY_KIND_SYSTEM_EVENT",
            "ENTRY_KIND_GOAL_UPDATE",
            "ENTRY_KIND_PLAN_STEP",
            "ENTRY_KIND_HANDOFF_MARKER",
            "ENTRY_KIND_DEFERRED_INTENT",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = EntryKind;

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
                    "ENTRY_KIND_UNSPECIFIED" => Ok(EntryKind::Unspecified),
                    "ENTRY_KIND_USER_MESSAGE" => Ok(EntryKind::UserMessage),
                    "ENTRY_KIND_ASSISTANT_MESSAGE" => Ok(EntryKind::AssistantMessage),
                    "ENTRY_KIND_TOOL_CALL" => Ok(EntryKind::ToolCall),
                    "ENTRY_KIND_TOOL_RESULT" => Ok(EntryKind::ToolResult),
                    "ENTRY_KIND_SYSTEM_EVENT" => Ok(EntryKind::SystemEvent),
                    "ENTRY_KIND_GOAL_UPDATE" => Ok(EntryKind::GoalUpdate),
                    "ENTRY_KIND_PLAN_STEP" => Ok(EntryKind::PlanStep),
                    "ENTRY_KIND_HANDOFF_MARKER" => Ok(EntryKind::HandoffMarker),
                    "ENTRY_KIND_DEFERRED_INTENT" => Ok(EntryKind::DeferredIntent),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for Locus {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "LOCUS_UNSPECIFIED",
            Self::Endpoint => "LOCUS_ENDPOINT",
            Self::Server => "LOCUS_SERVER",
            Self::Split => "LOCUS_SPLIT",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for Locus {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "LOCUS_UNSPECIFIED",
            "LOCUS_ENDPOINT",
            "LOCUS_SERVER",
            "LOCUS_SPLIT",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Locus;

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
                    "LOCUS_UNSPECIFIED" => Ok(Locus::Unspecified),
                    "LOCUS_ENDPOINT" => Ok(Locus::Endpoint),
                    "LOCUS_SERVER" => Ok(Locus::Server),
                    "LOCUS_SPLIT" => Ok(Locus::Split),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for SessionMeta {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.session_id.is_empty() {
            len += 1;
        }
        if !self.soul_id.is_empty() {
            len += 1;
        }
        if !self.user_id.is_empty() {
            len += 1;
        }
        if self.state != 0 {
            len += 1;
        }
        if !self.active_lease.is_empty() {
            len += 1;
        }
        if self.created_at.is_some() {
            len += 1;
        }
        if self.last_activity.is_some() {
            len += 1;
        }
        if !self.labels.is_empty() {
            len += 1;
        }
        if !self.org_id.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.context.SessionMeta", len)?;
        if !self.session_id.is_empty() {
            struct_ser.serialize_field("sessionId", &self.session_id)?;
        }
        if !self.soul_id.is_empty() {
            struct_ser.serialize_field("soulId", &self.soul_id)?;
        }
        if !self.user_id.is_empty() {
            struct_ser.serialize_field("userId", &self.user_id)?;
        }
        if self.state != 0 {
            let v = SessionState::try_from(self.state)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.state)))?;
            struct_ser.serialize_field("state", &v)?;
        }
        if !self.active_lease.is_empty() {
            struct_ser.serialize_field("activeLease", &self.active_lease)?;
        }
        if let Some(v) = self.created_at.as_ref() {
            struct_ser.serialize_field("createdAt", v)?;
        }
        if let Some(v) = self.last_activity.as_ref() {
            struct_ser.serialize_field("lastActivity", v)?;
        }
        if !self.labels.is_empty() {
            struct_ser.serialize_field("labels", &self.labels)?;
        }
        if !self.org_id.is_empty() {
            struct_ser.serialize_field("orgId", &self.org_id)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SessionMeta {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "session_id",
            "sessionId",
            "soul_id",
            "soulId",
            "user_id",
            "userId",
            "state",
            "active_lease",
            "activeLease",
            "created_at",
            "createdAt",
            "last_activity",
            "lastActivity",
            "labels",
            "org_id",
            "orgId",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            SessionId,
            SoulId,
            UserId,
            State,
            ActiveLease,
            CreatedAt,
            LastActivity,
            Labels,
            OrgId,
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
                            "sessionId" | "session_id" => Ok(GeneratedField::SessionId),
                            "soulId" | "soul_id" => Ok(GeneratedField::SoulId),
                            "userId" | "user_id" => Ok(GeneratedField::UserId),
                            "state" => Ok(GeneratedField::State),
                            "activeLease" | "active_lease" => Ok(GeneratedField::ActiveLease),
                            "createdAt" | "created_at" => Ok(GeneratedField::CreatedAt),
                            "lastActivity" | "last_activity" => Ok(GeneratedField::LastActivity),
                            "labels" => Ok(GeneratedField::Labels),
                            "orgId" | "org_id" => Ok(GeneratedField::OrgId),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SessionMeta;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.context.SessionMeta")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SessionMeta, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut session_id__ = None;
                let mut soul_id__ = None;
                let mut user_id__ = None;
                let mut state__ = None;
                let mut active_lease__ = None;
                let mut created_at__ = None;
                let mut last_activity__ = None;
                let mut labels__ = None;
                let mut org_id__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::SessionId => {
                            if session_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sessionId"));
                            }
                            session_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SoulId => {
                            if soul_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("soulId"));
                            }
                            soul_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::UserId => {
                            if user_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("userId"));
                            }
                            user_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::State => {
                            if state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("state"));
                            }
                            state__ = Some(map_.next_value::<SessionState>()? as i32);
                        }
                        GeneratedField::ActiveLease => {
                            if active_lease__.is_some() {
                                return Err(serde::de::Error::duplicate_field("activeLease"));
                            }
                            active_lease__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CreatedAt => {
                            if created_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("createdAt"));
                            }
                            created_at__ = map_.next_value()?;
                        }
                        GeneratedField::LastActivity => {
                            if last_activity__.is_some() {
                                return Err(serde::de::Error::duplicate_field("lastActivity"));
                            }
                            last_activity__ = map_.next_value()?;
                        }
                        GeneratedField::Labels => {
                            if labels__.is_some() {
                                return Err(serde::de::Error::duplicate_field("labels"));
                            }
                            labels__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::OrgId => {
                            if org_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("orgId"));
                            }
                            org_id__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(SessionMeta {
                    session_id: session_id__.unwrap_or_default(),
                    soul_id: soul_id__.unwrap_or_default(),
                    user_id: user_id__.unwrap_or_default(),
                    state: state__.unwrap_or_default(),
                    active_lease: active_lease__.unwrap_or_default(),
                    created_at: created_at__,
                    last_activity: last_activity__,
                    labels: labels__.unwrap_or_default(),
                    org_id: org_id__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.context.SessionMeta", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SessionState {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "SESSION_STATE_UNSPECIFIED",
            Self::Active => "SESSION_STATE_ACTIVE",
            Self::Suspended => "SESSION_STATE_SUSPENDED",
            Self::HandedOff => "SESSION_STATE_HANDED_OFF",
            Self::Completed => "SESSION_STATE_COMPLETED",
            Self::Archived => "SESSION_STATE_ARCHIVED",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for SessionState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "SESSION_STATE_UNSPECIFIED",
            "SESSION_STATE_ACTIVE",
            "SESSION_STATE_SUSPENDED",
            "SESSION_STATE_HANDED_OFF",
            "SESSION_STATE_COMPLETED",
            "SESSION_STATE_ARCHIVED",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SessionState;

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
                    "SESSION_STATE_UNSPECIFIED" => Ok(SessionState::Unspecified),
                    "SESSION_STATE_ACTIVE" => Ok(SessionState::Active),
                    "SESSION_STATE_SUSPENDED" => Ok(SessionState::Suspended),
                    "SESSION_STATE_HANDED_OFF" => Ok(SessionState::HandedOff),
                    "SESSION_STATE_COMPLETED" => Ok(SessionState::Completed),
                    "SESSION_STATE_ARCHIVED" => Ok(SessionState::Archived),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
