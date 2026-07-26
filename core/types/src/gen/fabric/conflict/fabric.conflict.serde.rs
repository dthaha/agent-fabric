// @generated
impl serde::Serialize for ClarifyingQuestion {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.question_text.is_empty() {
            len += 1;
        }
        if !self.options.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.conflict.ClarifyingQuestion", len)?;
        if !self.question_text.is_empty() {
            struct_ser.serialize_field("questionText", &self.question_text)?;
        }
        if !self.options.is_empty() {
            struct_ser.serialize_field("options", &self.options)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ClarifyingQuestion {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "question_text",
            "questionText",
            "options",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            QuestionText,
            Options,
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
                            "questionText" | "question_text" => Ok(GeneratedField::QuestionText),
                            "options" => Ok(GeneratedField::Options),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ClarifyingQuestion;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.conflict.ClarifyingQuestion")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ClarifyingQuestion, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut question_text__ = None;
                let mut options__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::QuestionText => {
                            if question_text__.is_some() {
                                return Err(serde::de::Error::duplicate_field("questionText"));
                            }
                            question_text__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Options => {
                            if options__.is_some() {
                                return Err(serde::de::Error::duplicate_field("options"));
                            }
                            options__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(ClarifyingQuestion {
                    question_text: question_text__.unwrap_or_default(),
                    options: options__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.conflict.ClarifyingQuestion", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ConflictPolicy {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.tool_category.is_empty() {
            len += 1;
        }
        if self.resolution != 0 {
            len += 1;
        }
        if self.auto_approve_threshold != 0. {
            len += 1;
        }
        if self.require_compensation_support {
            len += 1;
        }
        if self.fallback != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.conflict.ConflictPolicy", len)?;
        if !self.tool_category.is_empty() {
            struct_ser.serialize_field("toolCategory", &self.tool_category)?;
        }
        if self.resolution != 0 {
            let v = ConflictResolution::try_from(self.resolution)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.resolution)))?;
            struct_ser.serialize_field("resolution", &v)?;
        }
        if self.auto_approve_threshold != 0. {
            struct_ser.serialize_field("autoApproveThreshold", &self.auto_approve_threshold)?;
        }
        if self.require_compensation_support {
            struct_ser.serialize_field("requireCompensationSupport", &self.require_compensation_support)?;
        }
        if self.fallback != 0 {
            let v = ConflictResolution::try_from(self.fallback)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.fallback)))?;
            struct_ser.serialize_field("fallback", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ConflictPolicy {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "tool_category",
            "toolCategory",
            "resolution",
            "auto_approve_threshold",
            "autoApproveThreshold",
            "require_compensation_support",
            "requireCompensationSupport",
            "fallback",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ToolCategory,
            Resolution,
            AutoApproveThreshold,
            RequireCompensationSupport,
            Fallback,
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
                            "toolCategory" | "tool_category" => Ok(GeneratedField::ToolCategory),
                            "resolution" => Ok(GeneratedField::Resolution),
                            "autoApproveThreshold" | "auto_approve_threshold" => Ok(GeneratedField::AutoApproveThreshold),
                            "requireCompensationSupport" | "require_compensation_support" => Ok(GeneratedField::RequireCompensationSupport),
                            "fallback" => Ok(GeneratedField::Fallback),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ConflictPolicy;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.conflict.ConflictPolicy")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ConflictPolicy, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut tool_category__ = None;
                let mut resolution__ = None;
                let mut auto_approve_threshold__ = None;
                let mut require_compensation_support__ = None;
                let mut fallback__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ToolCategory => {
                            if tool_category__.is_some() {
                                return Err(serde::de::Error::duplicate_field("toolCategory"));
                            }
                            tool_category__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Resolution => {
                            if resolution__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resolution"));
                            }
                            resolution__ = Some(map_.next_value::<ConflictResolution>()? as i32);
                        }
                        GeneratedField::AutoApproveThreshold => {
                            if auto_approve_threshold__.is_some() {
                                return Err(serde::de::Error::duplicate_field("autoApproveThreshold"));
                            }
                            auto_approve_threshold__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::RequireCompensationSupport => {
                            if require_compensation_support__.is_some() {
                                return Err(serde::de::Error::duplicate_field("requireCompensationSupport"));
                            }
                            require_compensation_support__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Fallback => {
                            if fallback__.is_some() {
                                return Err(serde::de::Error::duplicate_field("fallback"));
                            }
                            fallback__ = Some(map_.next_value::<ConflictResolution>()? as i32);
                        }
                    }
                }
                Ok(ConflictPolicy {
                    tool_category: tool_category__.unwrap_or_default(),
                    resolution: resolution__.unwrap_or_default(),
                    auto_approve_threshold: auto_approve_threshold__.unwrap_or_default(),
                    require_compensation_support: require_compensation_support__.unwrap_or_default(),
                    fallback: fallback__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.conflict.ConflictPolicy", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ConflictRelation {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "CONFLICT_RELATION_UNSPECIFIED",
            Self::Supersedes => "CONFLICT_RELATION_SUPERSEDES",
            Self::Contradicts => "CONFLICT_RELATION_CONTRADICTS",
            Self::Independent => "CONFLICT_RELATION_INDEPENDENT",
            Self::Ambiguous => "CONFLICT_RELATION_AMBIGUOUS",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ConflictRelation {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "CONFLICT_RELATION_UNSPECIFIED",
            "CONFLICT_RELATION_SUPERSEDES",
            "CONFLICT_RELATION_CONTRADICTS",
            "CONFLICT_RELATION_INDEPENDENT",
            "CONFLICT_RELATION_AMBIGUOUS",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ConflictRelation;

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
                    "CONFLICT_RELATION_UNSPECIFIED" => Ok(ConflictRelation::Unspecified),
                    "CONFLICT_RELATION_SUPERSEDES" => Ok(ConflictRelation::Supersedes),
                    "CONFLICT_RELATION_CONTRADICTS" => Ok(ConflictRelation::Contradicts),
                    "CONFLICT_RELATION_INDEPENDENT" => Ok(ConflictRelation::Independent),
                    "CONFLICT_RELATION_AMBIGUOUS" => Ok(ConflictRelation::Ambiguous),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ConflictResolution {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "CONFLICT_RESOLUTION_UNSPECIFIED",
            Self::LastWriteWins => "CONFLICT_RESOLUTION_LAST_WRITE_WINS",
            Self::Compensate => "CONFLICT_RESOLUTION_COMPENSATE",
            Self::Escalate => "CONFLICT_RESOLUTION_ESCALATE",
            Self::Quarantine => "CONFLICT_RESOLUTION_QUARANTINE",
            Self::Rollback => "CONFLICT_RESOLUTION_ROLLBACK",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ConflictResolution {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "CONFLICT_RESOLUTION_UNSPECIFIED",
            "CONFLICT_RESOLUTION_LAST_WRITE_WINS",
            "CONFLICT_RESOLUTION_COMPENSATE",
            "CONFLICT_RESOLUTION_ESCALATE",
            "CONFLICT_RESOLUTION_QUARANTINE",
            "CONFLICT_RESOLUTION_ROLLBACK",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ConflictResolution;

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
                    "CONFLICT_RESOLUTION_UNSPECIFIED" => Ok(ConflictResolution::Unspecified),
                    "CONFLICT_RESOLUTION_LAST_WRITE_WINS" => Ok(ConflictResolution::LastWriteWins),
                    "CONFLICT_RESOLUTION_COMPENSATE" => Ok(ConflictResolution::Compensate),
                    "CONFLICT_RESOLUTION_ESCALATE" => Ok(ConflictResolution::Escalate),
                    "CONFLICT_RESOLUTION_QUARANTINE" => Ok(ConflictResolution::Quarantine),
                    "CONFLICT_RESOLUTION_ROLLBACK" => Ok(ConflictResolution::Rollback),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ConflictVerdict {
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
        if !self.entry_id_a.is_empty() {
            len += 1;
        }
        if !self.entry_id_b.is_empty() {
            len += 1;
        }
        if self.relation != 0 {
            len += 1;
        }
        if !self.shared_entities.is_empty() {
            len += 1;
        }
        if self.confidence != 0. {
            len += 1;
        }
        if !self.explanation.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.conflict.ConflictVerdict", len)?;
        if !self.session_id.is_empty() {
            struct_ser.serialize_field("sessionId", &self.session_id)?;
        }
        if !self.entry_id_a.is_empty() {
            struct_ser.serialize_field("entryIdA", &self.entry_id_a)?;
        }
        if !self.entry_id_b.is_empty() {
            struct_ser.serialize_field("entryIdB", &self.entry_id_b)?;
        }
        if self.relation != 0 {
            let v = ConflictRelation::try_from(self.relation)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.relation)))?;
            struct_ser.serialize_field("relation", &v)?;
        }
        if !self.shared_entities.is_empty() {
            struct_ser.serialize_field("sharedEntities", &self.shared_entities)?;
        }
        if self.confidence != 0. {
            struct_ser.serialize_field("confidence", &self.confidence)?;
        }
        if !self.explanation.is_empty() {
            struct_ser.serialize_field("explanation", &self.explanation)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ConflictVerdict {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "session_id",
            "sessionId",
            "entry_id_a",
            "entryIdA",
            "entry_id_b",
            "entryIdB",
            "relation",
            "shared_entities",
            "sharedEntities",
            "confidence",
            "explanation",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            SessionId,
            EntryIdA,
            EntryIdB,
            Relation,
            SharedEntities,
            Confidence,
            Explanation,
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
                            "entryIdA" | "entry_id_a" => Ok(GeneratedField::EntryIdA),
                            "entryIdB" | "entry_id_b" => Ok(GeneratedField::EntryIdB),
                            "relation" => Ok(GeneratedField::Relation),
                            "sharedEntities" | "shared_entities" => Ok(GeneratedField::SharedEntities),
                            "confidence" => Ok(GeneratedField::Confidence),
                            "explanation" => Ok(GeneratedField::Explanation),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ConflictVerdict;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.conflict.ConflictVerdict")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ConflictVerdict, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut session_id__ = None;
                let mut entry_id_a__ = None;
                let mut entry_id_b__ = None;
                let mut relation__ = None;
                let mut shared_entities__ = None;
                let mut confidence__ = None;
                let mut explanation__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::SessionId => {
                            if session_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sessionId"));
                            }
                            session_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EntryIdA => {
                            if entry_id_a__.is_some() {
                                return Err(serde::de::Error::duplicate_field("entryIdA"));
                            }
                            entry_id_a__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EntryIdB => {
                            if entry_id_b__.is_some() {
                                return Err(serde::de::Error::duplicate_field("entryIdB"));
                            }
                            entry_id_b__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Relation => {
                            if relation__.is_some() {
                                return Err(serde::de::Error::duplicate_field("relation"));
                            }
                            relation__ = Some(map_.next_value::<ConflictRelation>()? as i32);
                        }
                        GeneratedField::SharedEntities => {
                            if shared_entities__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sharedEntities"));
                            }
                            shared_entities__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Confidence => {
                            if confidence__.is_some() {
                                return Err(serde::de::Error::duplicate_field("confidence"));
                            }
                            confidence__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Explanation => {
                            if explanation__.is_some() {
                                return Err(serde::de::Error::duplicate_field("explanation"));
                            }
                            explanation__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(ConflictVerdict {
                    session_id: session_id__.unwrap_or_default(),
                    entry_id_a: entry_id_a__.unwrap_or_default(),
                    entry_id_b: entry_id_b__.unwrap_or_default(),
                    relation: relation__.unwrap_or_default(),
                    shared_entities: shared_entities__.unwrap_or_default(),
                    confidence: confidence__.unwrap_or_default(),
                    explanation: explanation__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.conflict.ConflictVerdict", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ResolutionProposal {
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
        if self.relation != 0 {
            len += 1;
        }
        if !self.winning_entry_id.is_empty() {
            len += 1;
        }
        if self.proposed_resolution != 0 {
            len += 1;
        }
        if self.confidence != 0. {
            len += 1;
        }
        if !self.rationale.is_empty() {
            len += 1;
        }
        if self.clarifying_question.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.conflict.ResolutionProposal", len)?;
        if !self.session_id.is_empty() {
            struct_ser.serialize_field("sessionId", &self.session_id)?;
        }
        if self.relation != 0 {
            let v = ConflictRelation::try_from(self.relation)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.relation)))?;
            struct_ser.serialize_field("relation", &v)?;
        }
        if !self.winning_entry_id.is_empty() {
            struct_ser.serialize_field("winningEntryId", &self.winning_entry_id)?;
        }
        if self.proposed_resolution != 0 {
            let v = ConflictResolution::try_from(self.proposed_resolution)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.proposed_resolution)))?;
            struct_ser.serialize_field("proposedResolution", &v)?;
        }
        if self.confidence != 0. {
            struct_ser.serialize_field("confidence", &self.confidence)?;
        }
        if !self.rationale.is_empty() {
            struct_ser.serialize_field("rationale", &self.rationale)?;
        }
        if let Some(v) = self.clarifying_question.as_ref() {
            struct_ser.serialize_field("clarifyingQuestion", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ResolutionProposal {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "session_id",
            "sessionId",
            "relation",
            "winning_entry_id",
            "winningEntryId",
            "proposed_resolution",
            "proposedResolution",
            "confidence",
            "rationale",
            "clarifying_question",
            "clarifyingQuestion",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            SessionId,
            Relation,
            WinningEntryId,
            ProposedResolution,
            Confidence,
            Rationale,
            ClarifyingQuestion,
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
                            "relation" => Ok(GeneratedField::Relation),
                            "winningEntryId" | "winning_entry_id" => Ok(GeneratedField::WinningEntryId),
                            "proposedResolution" | "proposed_resolution" => Ok(GeneratedField::ProposedResolution),
                            "confidence" => Ok(GeneratedField::Confidence),
                            "rationale" => Ok(GeneratedField::Rationale),
                            "clarifyingQuestion" | "clarifying_question" => Ok(GeneratedField::ClarifyingQuestion),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ResolutionProposal;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.conflict.ResolutionProposal")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ResolutionProposal, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut session_id__ = None;
                let mut relation__ = None;
                let mut winning_entry_id__ = None;
                let mut proposed_resolution__ = None;
                let mut confidence__ = None;
                let mut rationale__ = None;
                let mut clarifying_question__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::SessionId => {
                            if session_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sessionId"));
                            }
                            session_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Relation => {
                            if relation__.is_some() {
                                return Err(serde::de::Error::duplicate_field("relation"));
                            }
                            relation__ = Some(map_.next_value::<ConflictRelation>()? as i32);
                        }
                        GeneratedField::WinningEntryId => {
                            if winning_entry_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("winningEntryId"));
                            }
                            winning_entry_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ProposedResolution => {
                            if proposed_resolution__.is_some() {
                                return Err(serde::de::Error::duplicate_field("proposedResolution"));
                            }
                            proposed_resolution__ = Some(map_.next_value::<ConflictResolution>()? as i32);
                        }
                        GeneratedField::Confidence => {
                            if confidence__.is_some() {
                                return Err(serde::de::Error::duplicate_field("confidence"));
                            }
                            confidence__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Rationale => {
                            if rationale__.is_some() {
                                return Err(serde::de::Error::duplicate_field("rationale"));
                            }
                            rationale__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ClarifyingQuestion => {
                            if clarifying_question__.is_some() {
                                return Err(serde::de::Error::duplicate_field("clarifyingQuestion"));
                            }
                            clarifying_question__ = map_.next_value()?;
                        }
                    }
                }
                Ok(ResolutionProposal {
                    session_id: session_id__.unwrap_or_default(),
                    relation: relation__.unwrap_or_default(),
                    winning_entry_id: winning_entry_id__.unwrap_or_default(),
                    proposed_resolution: proposed_resolution__.unwrap_or_default(),
                    confidence: confidence__.unwrap_or_default(),
                    rationale: rationale__.unwrap_or_default(),
                    clarifying_question: clarifying_question__,
                })
            }
        }
        deserializer.deserialize_struct("fabric.conflict.ResolutionProposal", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SharedEntity {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.entity_type.is_empty() {
            len += 1;
        }
        if !self.entity_id.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.conflict.SharedEntity", len)?;
        if !self.entity_type.is_empty() {
            struct_ser.serialize_field("entityType", &self.entity_type)?;
        }
        if !self.entity_id.is_empty() {
            struct_ser.serialize_field("entityId", &self.entity_id)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SharedEntity {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "entity_type",
            "entityType",
            "entity_id",
            "entityId",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            EntityType,
            EntityId,
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
                            "entityType" | "entity_type" => Ok(GeneratedField::EntityType),
                            "entityId" | "entity_id" => Ok(GeneratedField::EntityId),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SharedEntity;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.conflict.SharedEntity")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SharedEntity, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut entity_type__ = None;
                let mut entity_id__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::EntityType => {
                            if entity_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("entityType"));
                            }
                            entity_type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EntityId => {
                            if entity_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("entityId"));
                            }
                            entity_id__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(SharedEntity {
                    entity_type: entity_type__.unwrap_or_default(),
                    entity_id: entity_id__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.conflict.SharedEntity", FIELDS, GeneratedVisitor)
    }
}
