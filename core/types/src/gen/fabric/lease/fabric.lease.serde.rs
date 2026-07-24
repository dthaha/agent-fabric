// @generated
impl serde::Serialize for HandoffAck {
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
        if !self.new_holder.is_empty() {
            len += 1;
        }
        if self.caught_up_to_seq != 0 {
            len += 1;
        }
        if self.success {
            len += 1;
        }
        if !self.error.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.lease.HandoffAck", len)?;
        if !self.session_id.is_empty() {
            struct_ser.serialize_field("sessionId", &self.session_id)?;
        }
        if !self.new_holder.is_empty() {
            struct_ser.serialize_field("newHolder", &self.new_holder)?;
        }
        if self.caught_up_to_seq != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("caughtUpToSeq", ToString::to_string(&self.caught_up_to_seq).as_str())?;
        }
        if self.success {
            struct_ser.serialize_field("success", &self.success)?;
        }
        if !self.error.is_empty() {
            struct_ser.serialize_field("error", &self.error)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for HandoffAck {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "session_id",
            "sessionId",
            "new_holder",
            "newHolder",
            "caught_up_to_seq",
            "caughtUpToSeq",
            "success",
            "error",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            SessionId,
            NewHolder,
            CaughtUpToSeq,
            Success,
            Error,
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
                            "newHolder" | "new_holder" => Ok(GeneratedField::NewHolder),
                            "caughtUpToSeq" | "caught_up_to_seq" => Ok(GeneratedField::CaughtUpToSeq),
                            "success" => Ok(GeneratedField::Success),
                            "error" => Ok(GeneratedField::Error),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = HandoffAck;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.lease.HandoffAck")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<HandoffAck, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut session_id__ = None;
                let mut new_holder__ = None;
                let mut caught_up_to_seq__ = None;
                let mut success__ = None;
                let mut error__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::SessionId => {
                            if session_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sessionId"));
                            }
                            session_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::NewHolder => {
                            if new_holder__.is_some() {
                                return Err(serde::de::Error::duplicate_field("newHolder"));
                            }
                            new_holder__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CaughtUpToSeq => {
                            if caught_up_to_seq__.is_some() {
                                return Err(serde::de::Error::duplicate_field("caughtUpToSeq"));
                            }
                            caught_up_to_seq__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Success => {
                            if success__.is_some() {
                                return Err(serde::de::Error::duplicate_field("success"));
                            }
                            success__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Error => {
                            if error__.is_some() {
                                return Err(serde::de::Error::duplicate_field("error"));
                            }
                            error__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(HandoffAck {
                    session_id: session_id__.unwrap_or_default(),
                    new_holder: new_holder__.unwrap_or_default(),
                    caught_up_to_seq: caught_up_to_seq__.unwrap_or_default(),
                    success: success__.unwrap_or_default(),
                    error: error__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.lease.HandoffAck", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for HandoffRequest {
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
        if !self.from_holder.is_empty() {
            len += 1;
        }
        if !self.to_holder.is_empty() {
            len += 1;
        }
        if self.freeze_at_seq != 0 {
            len += 1;
        }
        if !self.reason.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.lease.HandoffRequest", len)?;
        if !self.session_id.is_empty() {
            struct_ser.serialize_field("sessionId", &self.session_id)?;
        }
        if !self.from_holder.is_empty() {
            struct_ser.serialize_field("fromHolder", &self.from_holder)?;
        }
        if !self.to_holder.is_empty() {
            struct_ser.serialize_field("toHolder", &self.to_holder)?;
        }
        if self.freeze_at_seq != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("freezeAtSeq", ToString::to_string(&self.freeze_at_seq).as_str())?;
        }
        if !self.reason.is_empty() {
            struct_ser.serialize_field("reason", &self.reason)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for HandoffRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "session_id",
            "sessionId",
            "from_holder",
            "fromHolder",
            "to_holder",
            "toHolder",
            "freeze_at_seq",
            "freezeAtSeq",
            "reason",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            SessionId,
            FromHolder,
            ToHolder,
            FreezeAtSeq,
            Reason,
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
                            "fromHolder" | "from_holder" => Ok(GeneratedField::FromHolder),
                            "toHolder" | "to_holder" => Ok(GeneratedField::ToHolder),
                            "freezeAtSeq" | "freeze_at_seq" => Ok(GeneratedField::FreezeAtSeq),
                            "reason" => Ok(GeneratedField::Reason),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = HandoffRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.lease.HandoffRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<HandoffRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut session_id__ = None;
                let mut from_holder__ = None;
                let mut to_holder__ = None;
                let mut freeze_at_seq__ = None;
                let mut reason__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::SessionId => {
                            if session_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sessionId"));
                            }
                            session_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::FromHolder => {
                            if from_holder__.is_some() {
                                return Err(serde::de::Error::duplicate_field("fromHolder"));
                            }
                            from_holder__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ToHolder => {
                            if to_holder__.is_some() {
                                return Err(serde::de::Error::duplicate_field("toHolder"));
                            }
                            to_holder__ = Some(map_.next_value()?);
                        }
                        GeneratedField::FreezeAtSeq => {
                            if freeze_at_seq__.is_some() {
                                return Err(serde::de::Error::duplicate_field("freezeAtSeq"));
                            }
                            freeze_at_seq__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Reason => {
                            if reason__.is_some() {
                                return Err(serde::de::Error::duplicate_field("reason"));
                            }
                            reason__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(HandoffRequest {
                    session_id: session_id__.unwrap_or_default(),
                    from_holder: from_holder__.unwrap_or_default(),
                    to_holder: to_holder__.unwrap_or_default(),
                    freeze_at_seq: freeze_at_seq__.unwrap_or_default(),
                    reason: reason__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.lease.HandoffRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Lease {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.lease_id.is_empty() {
            len += 1;
        }
        if !self.session_id.is_empty() {
            len += 1;
        }
        if !self.holder_id.is_empty() {
            len += 1;
        }
        if self.locus != 0 {
            len += 1;
        }
        if self.granted_seq != 0 {
            len += 1;
        }
        if self.granted_at.is_some() {
            len += 1;
        }
        if self.expires_at.is_some() {
            len += 1;
        }
        if self.state != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.lease.Lease", len)?;
        if !self.lease_id.is_empty() {
            struct_ser.serialize_field("leaseId", &self.lease_id)?;
        }
        if !self.session_id.is_empty() {
            struct_ser.serialize_field("sessionId", &self.session_id)?;
        }
        if !self.holder_id.is_empty() {
            struct_ser.serialize_field("holderId", &self.holder_id)?;
        }
        if self.locus != 0 {
            let v = super::context::Locus::try_from(self.locus)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.locus)))?;
            struct_ser.serialize_field("locus", &v)?;
        }
        if self.granted_seq != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("grantedSeq", ToString::to_string(&self.granted_seq).as_str())?;
        }
        if let Some(v) = self.granted_at.as_ref() {
            struct_ser.serialize_field("grantedAt", v)?;
        }
        if let Some(v) = self.expires_at.as_ref() {
            struct_ser.serialize_field("expiresAt", v)?;
        }
        if self.state != 0 {
            let v = LeaseState::try_from(self.state)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.state)))?;
            struct_ser.serialize_field("state", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Lease {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "lease_id",
            "leaseId",
            "session_id",
            "sessionId",
            "holder_id",
            "holderId",
            "locus",
            "granted_seq",
            "grantedSeq",
            "granted_at",
            "grantedAt",
            "expires_at",
            "expiresAt",
            "state",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            LeaseId,
            SessionId,
            HolderId,
            Locus,
            GrantedSeq,
            GrantedAt,
            ExpiresAt,
            State,
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
                            "leaseId" | "lease_id" => Ok(GeneratedField::LeaseId),
                            "sessionId" | "session_id" => Ok(GeneratedField::SessionId),
                            "holderId" | "holder_id" => Ok(GeneratedField::HolderId),
                            "locus" => Ok(GeneratedField::Locus),
                            "grantedSeq" | "granted_seq" => Ok(GeneratedField::GrantedSeq),
                            "grantedAt" | "granted_at" => Ok(GeneratedField::GrantedAt),
                            "expiresAt" | "expires_at" => Ok(GeneratedField::ExpiresAt),
                            "state" => Ok(GeneratedField::State),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Lease;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.lease.Lease")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Lease, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut lease_id__ = None;
                let mut session_id__ = None;
                let mut holder_id__ = None;
                let mut locus__ = None;
                let mut granted_seq__ = None;
                let mut granted_at__ = None;
                let mut expires_at__ = None;
                let mut state__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::LeaseId => {
                            if lease_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("leaseId"));
                            }
                            lease_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SessionId => {
                            if session_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sessionId"));
                            }
                            session_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::HolderId => {
                            if holder_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("holderId"));
                            }
                            holder_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Locus => {
                            if locus__.is_some() {
                                return Err(serde::de::Error::duplicate_field("locus"));
                            }
                            locus__ = Some(map_.next_value::<super::context::Locus>()? as i32);
                        }
                        GeneratedField::GrantedSeq => {
                            if granted_seq__.is_some() {
                                return Err(serde::de::Error::duplicate_field("grantedSeq"));
                            }
                            granted_seq__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::GrantedAt => {
                            if granted_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("grantedAt"));
                            }
                            granted_at__ = map_.next_value()?;
                        }
                        GeneratedField::ExpiresAt => {
                            if expires_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("expiresAt"));
                            }
                            expires_at__ = map_.next_value()?;
                        }
                        GeneratedField::State => {
                            if state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("state"));
                            }
                            state__ = Some(map_.next_value::<LeaseState>()? as i32);
                        }
                    }
                }
                Ok(Lease {
                    lease_id: lease_id__.unwrap_or_default(),
                    session_id: session_id__.unwrap_or_default(),
                    holder_id: holder_id__.unwrap_or_default(),
                    locus: locus__.unwrap_or_default(),
                    granted_seq: granted_seq__.unwrap_or_default(),
                    granted_at: granted_at__,
                    expires_at: expires_at__,
                    state: state__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.lease.Lease", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for LeaseState {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "LEASE_STATE_UNSPECIFIED",
            Self::Active => "LEASE_STATE_ACTIVE",
            Self::Expired => "LEASE_STATE_EXPIRED",
            Self::Revoked => "LEASE_STATE_REVOKED",
            Self::Transferred => "LEASE_STATE_TRANSFERRED",
            Self::Released => "LEASE_STATE_RELEASED",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for LeaseState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "LEASE_STATE_UNSPECIFIED",
            "LEASE_STATE_ACTIVE",
            "LEASE_STATE_EXPIRED",
            "LEASE_STATE_REVOKED",
            "LEASE_STATE_TRANSFERRED",
            "LEASE_STATE_RELEASED",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = LeaseState;

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
                    "LEASE_STATE_UNSPECIFIED" => Ok(LeaseState::Unspecified),
                    "LEASE_STATE_ACTIVE" => Ok(LeaseState::Active),
                    "LEASE_STATE_EXPIRED" => Ok(LeaseState::Expired),
                    "LEASE_STATE_REVOKED" => Ok(LeaseState::Revoked),
                    "LEASE_STATE_TRANSFERRED" => Ok(LeaseState::Transferred),
                    "LEASE_STATE_RELEASED" => Ok(LeaseState::Released),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
