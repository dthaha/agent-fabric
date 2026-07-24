// @generated
impl serde::Serialize for InstallStatus {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "INSTALL_STATUS_UNSPECIFIED",
            Self::Pending => "INSTALL_STATUS_PENDING",
            Self::Downloading => "INSTALL_STATUS_DOWNLOADING",
            Self::Verifying => "INSTALL_STATUS_VERIFYING",
            Self::Active => "INSTALL_STATUS_ACTIVE",
            Self::Retired => "INSTALL_STATUS_RETIRED",
            Self::Failed => "INSTALL_STATUS_FAILED",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for InstallStatus {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "INSTALL_STATUS_UNSPECIFIED",
            "INSTALL_STATUS_PENDING",
            "INSTALL_STATUS_DOWNLOADING",
            "INSTALL_STATUS_VERIFYING",
            "INSTALL_STATUS_ACTIVE",
            "INSTALL_STATUS_RETIRED",
            "INSTALL_STATUS_FAILED",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = InstallStatus;

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
                    "INSTALL_STATUS_UNSPECIFIED" => Ok(InstallStatus::Unspecified),
                    "INSTALL_STATUS_PENDING" => Ok(InstallStatus::Pending),
                    "INSTALL_STATUS_DOWNLOADING" => Ok(InstallStatus::Downloading),
                    "INSTALL_STATUS_VERIFYING" => Ok(InstallStatus::Verifying),
                    "INSTALL_STATUS_ACTIVE" => Ok(InstallStatus::Active),
                    "INSTALL_STATUS_RETIRED" => Ok(InstallStatus::Retired),
                    "INSTALL_STATUS_FAILED" => Ok(InstallStatus::Failed),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for InstalledModel {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.model_id.is_empty() {
            len += 1;
        }
        if !self.variant_id.is_empty() {
            len += 1;
        }
        if !self.version.is_empty() {
            len += 1;
        }
        if self.status != 0 {
            len += 1;
        }
        if self.sha256_verified {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.catalog.InstalledModel", len)?;
        if !self.model_id.is_empty() {
            struct_ser.serialize_field("modelId", &self.model_id)?;
        }
        if !self.variant_id.is_empty() {
            struct_ser.serialize_field("variantId", &self.variant_id)?;
        }
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        if self.status != 0 {
            let v = InstallStatus::try_from(self.status)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.status)))?;
            struct_ser.serialize_field("status", &v)?;
        }
        if self.sha256_verified {
            struct_ser.serialize_field("sha256Verified", &self.sha256_verified)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for InstalledModel {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "model_id",
            "modelId",
            "variant_id",
            "variantId",
            "version",
            "status",
            "sha256_verified",
            "sha256Verified",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ModelId,
            VariantId,
            Version,
            Status,
            Sha256Verified,
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
                            "modelId" | "model_id" => Ok(GeneratedField::ModelId),
                            "variantId" | "variant_id" => Ok(GeneratedField::VariantId),
                            "version" => Ok(GeneratedField::Version),
                            "status" => Ok(GeneratedField::Status),
                            "sha256Verified" | "sha256_verified" => Ok(GeneratedField::Sha256Verified),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = InstalledModel;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.catalog.InstalledModel")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<InstalledModel, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut model_id__ = None;
                let mut variant_id__ = None;
                let mut version__ = None;
                let mut status__ = None;
                let mut sha256_verified__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ModelId => {
                            if model_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("modelId"));
                            }
                            model_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::VariantId => {
                            if variant_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("variantId"));
                            }
                            variant_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Status => {
                            if status__.is_some() {
                                return Err(serde::de::Error::duplicate_field("status"));
                            }
                            status__ = Some(map_.next_value::<InstallStatus>()? as i32);
                        }
                        GeneratedField::Sha256Verified => {
                            if sha256_verified__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sha256Verified"));
                            }
                            sha256_verified__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(InstalledModel {
                    model_id: model_id__.unwrap_or_default(),
                    variant_id: variant_id__.unwrap_or_default(),
                    version: version__.unwrap_or_default(),
                    status: status__.unwrap_or_default(),
                    sha256_verified: sha256_verified__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.catalog.InstalledModel", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for LogicalModel {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.model_id.is_empty() {
            len += 1;
        }
        if !self.version.is_empty() {
            len += 1;
        }
        if !self.capabilities.is_empty() {
            len += 1;
        }
        if self.context_window != 0 {
            len += 1;
        }
        if !self.license.is_empty() {
            len += 1;
        }
        if !self.variants.is_empty() {
            len += 1;
        }
        if self.tool_support != 0 {
            len += 1;
        }
        if self.quality_tier != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.catalog.LogicalModel", len)?;
        if !self.model_id.is_empty() {
            struct_ser.serialize_field("modelId", &self.model_id)?;
        }
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        if !self.capabilities.is_empty() {
            struct_ser.serialize_field("capabilities", &self.capabilities)?;
        }
        if self.context_window != 0 {
            struct_ser.serialize_field("contextWindow", &self.context_window)?;
        }
        if !self.license.is_empty() {
            struct_ser.serialize_field("license", &self.license)?;
        }
        if !self.variants.is_empty() {
            struct_ser.serialize_field("variants", &self.variants)?;
        }
        if self.tool_support != 0 {
            let v = ToolSupport::try_from(self.tool_support)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.tool_support)))?;
            struct_ser.serialize_field("toolSupport", &v)?;
        }
        if self.quality_tier != 0 {
            let v = QualityTier::try_from(self.quality_tier)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.quality_tier)))?;
            struct_ser.serialize_field("qualityTier", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for LogicalModel {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "model_id",
            "modelId",
            "version",
            "capabilities",
            "context_window",
            "contextWindow",
            "license",
            "variants",
            "tool_support",
            "toolSupport",
            "quality_tier",
            "qualityTier",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ModelId,
            Version,
            Capabilities,
            ContextWindow,
            License,
            Variants,
            ToolSupport,
            QualityTier,
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
                            "modelId" | "model_id" => Ok(GeneratedField::ModelId),
                            "version" => Ok(GeneratedField::Version),
                            "capabilities" => Ok(GeneratedField::Capabilities),
                            "contextWindow" | "context_window" => Ok(GeneratedField::ContextWindow),
                            "license" => Ok(GeneratedField::License),
                            "variants" => Ok(GeneratedField::Variants),
                            "toolSupport" | "tool_support" => Ok(GeneratedField::ToolSupport),
                            "qualityTier" | "quality_tier" => Ok(GeneratedField::QualityTier),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = LogicalModel;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.catalog.LogicalModel")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<LogicalModel, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut model_id__ = None;
                let mut version__ = None;
                let mut capabilities__ = None;
                let mut context_window__ = None;
                let mut license__ = None;
                let mut variants__ = None;
                let mut tool_support__ = None;
                let mut quality_tier__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ModelId => {
                            if model_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("modelId"));
                            }
                            model_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Capabilities => {
                            if capabilities__.is_some() {
                                return Err(serde::de::Error::duplicate_field("capabilities"));
                            }
                            capabilities__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ContextWindow => {
                            if context_window__.is_some() {
                                return Err(serde::de::Error::duplicate_field("contextWindow"));
                            }
                            context_window__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::License => {
                            if license__.is_some() {
                                return Err(serde::de::Error::duplicate_field("license"));
                            }
                            license__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Variants => {
                            if variants__.is_some() {
                                return Err(serde::de::Error::duplicate_field("variants"));
                            }
                            variants__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ToolSupport => {
                            if tool_support__.is_some() {
                                return Err(serde::de::Error::duplicate_field("toolSupport"));
                            }
                            tool_support__ = Some(map_.next_value::<ToolSupport>()? as i32);
                        }
                        GeneratedField::QualityTier => {
                            if quality_tier__.is_some() {
                                return Err(serde::de::Error::duplicate_field("qualityTier"));
                            }
                            quality_tier__ = Some(map_.next_value::<QualityTier>()? as i32);
                        }
                    }
                }
                Ok(LogicalModel {
                    model_id: model_id__.unwrap_or_default(),
                    version: version__.unwrap_or_default(),
                    capabilities: capabilities__.unwrap_or_default(),
                    context_window: context_window__.unwrap_or_default(),
                    license: license__.unwrap_or_default(),
                    variants: variants__.unwrap_or_default(),
                    tool_support: tool_support__.unwrap_or_default(),
                    quality_tier: quality_tier__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.catalog.LogicalModel", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ModelPack {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.pack_id.is_empty() {
            len += 1;
        }
        if !self.name.is_empty() {
            len += 1;
        }
        if !self.logical_model_ids.is_empty() {
            len += 1;
        }
        if self.disk_budget_mb != 0 {
            len += 1;
        }
        if !self.device_classes.is_empty() {
            len += 1;
        }
        if !self.org_groups.is_empty() {
            len += 1;
        }
        if self.includes_classifier {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.catalog.ModelPack", len)?;
        if !self.pack_id.is_empty() {
            struct_ser.serialize_field("packId", &self.pack_id)?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.logical_model_ids.is_empty() {
            struct_ser.serialize_field("logicalModelIds", &self.logical_model_ids)?;
        }
        if self.disk_budget_mb != 0 {
            struct_ser.serialize_field("diskBudgetMb", &self.disk_budget_mb)?;
        }
        if !self.device_classes.is_empty() {
            struct_ser.serialize_field("deviceClasses", &self.device_classes)?;
        }
        if !self.org_groups.is_empty() {
            struct_ser.serialize_field("orgGroups", &self.org_groups)?;
        }
        if self.includes_classifier {
            struct_ser.serialize_field("includesClassifier", &self.includes_classifier)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ModelPack {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "pack_id",
            "packId",
            "name",
            "logical_model_ids",
            "logicalModelIds",
            "disk_budget_mb",
            "diskBudgetMb",
            "device_classes",
            "deviceClasses",
            "org_groups",
            "orgGroups",
            "includes_classifier",
            "includesClassifier",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            PackId,
            Name,
            LogicalModelIds,
            DiskBudgetMb,
            DeviceClasses,
            OrgGroups,
            IncludesClassifier,
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
                            "packId" | "pack_id" => Ok(GeneratedField::PackId),
                            "name" => Ok(GeneratedField::Name),
                            "logicalModelIds" | "logical_model_ids" => Ok(GeneratedField::LogicalModelIds),
                            "diskBudgetMb" | "disk_budget_mb" => Ok(GeneratedField::DiskBudgetMb),
                            "deviceClasses" | "device_classes" => Ok(GeneratedField::DeviceClasses),
                            "orgGroups" | "org_groups" => Ok(GeneratedField::OrgGroups),
                            "includesClassifier" | "includes_classifier" => Ok(GeneratedField::IncludesClassifier),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ModelPack;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.catalog.ModelPack")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ModelPack, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut pack_id__ = None;
                let mut name__ = None;
                let mut logical_model_ids__ = None;
                let mut disk_budget_mb__ = None;
                let mut device_classes__ = None;
                let mut org_groups__ = None;
                let mut includes_classifier__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::PackId => {
                            if pack_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("packId"));
                            }
                            pack_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::LogicalModelIds => {
                            if logical_model_ids__.is_some() {
                                return Err(serde::de::Error::duplicate_field("logicalModelIds"));
                            }
                            logical_model_ids__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DiskBudgetMb => {
                            if disk_budget_mb__.is_some() {
                                return Err(serde::de::Error::duplicate_field("diskBudgetMb"));
                            }
                            disk_budget_mb__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DeviceClasses => {
                            if device_classes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("deviceClasses"));
                            }
                            device_classes__ = Some(map_.next_value()?);
                        }
                        GeneratedField::OrgGroups => {
                            if org_groups__.is_some() {
                                return Err(serde::de::Error::duplicate_field("orgGroups"));
                            }
                            org_groups__ = Some(map_.next_value()?);
                        }
                        GeneratedField::IncludesClassifier => {
                            if includes_classifier__.is_some() {
                                return Err(serde::de::Error::duplicate_field("includesClassifier"));
                            }
                            includes_classifier__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(ModelPack {
                    pack_id: pack_id__.unwrap_or_default(),
                    name: name__.unwrap_or_default(),
                    logical_model_ids: logical_model_ids__.unwrap_or_default(),
                    disk_budget_mb: disk_budget_mb__.unwrap_or_default(),
                    device_classes: device_classes__.unwrap_or_default(),
                    org_groups: org_groups__.unwrap_or_default(),
                    includes_classifier: includes_classifier__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.catalog.ModelPack", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ModelVariant {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.variant_id.is_empty() {
            len += 1;
        }
        if self.runtime != 0 {
            len += 1;
        }
        if !self.arch.is_empty() {
            len += 1;
        }
        if !self.format.is_empty() {
            len += 1;
        }
        if !self.quant.is_empty() {
            len += 1;
        }
        if !self.uri.is_empty() {
            len += 1;
        }
        if !self.sha256.is_empty() {
            len += 1;
        }
        if self.size_bytes != 0 {
            len += 1;
        }
        if self.min_ram_gb != 0 {
            len += 1;
        }
        if !self.execution_providers.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.catalog.ModelVariant", len)?;
        if !self.variant_id.is_empty() {
            struct_ser.serialize_field("variantId", &self.variant_id)?;
        }
        if self.runtime != 0 {
            let v = Runtime::try_from(self.runtime)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.runtime)))?;
            struct_ser.serialize_field("runtime", &v)?;
        }
        if !self.arch.is_empty() {
            struct_ser.serialize_field("arch", &self.arch)?;
        }
        if !self.format.is_empty() {
            struct_ser.serialize_field("format", &self.format)?;
        }
        if !self.quant.is_empty() {
            struct_ser.serialize_field("quant", &self.quant)?;
        }
        if !self.uri.is_empty() {
            struct_ser.serialize_field("uri", &self.uri)?;
        }
        if !self.sha256.is_empty() {
            struct_ser.serialize_field("sha256", &self.sha256)?;
        }
        if self.size_bytes != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("sizeBytes", ToString::to_string(&self.size_bytes).as_str())?;
        }
        if self.min_ram_gb != 0 {
            struct_ser.serialize_field("minRamGb", &self.min_ram_gb)?;
        }
        if !self.execution_providers.is_empty() {
            struct_ser.serialize_field("executionProviders", &self.execution_providers)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ModelVariant {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "variant_id",
            "variantId",
            "runtime",
            "arch",
            "format",
            "quant",
            "uri",
            "sha256",
            "size_bytes",
            "sizeBytes",
            "min_ram_gb",
            "minRamGb",
            "execution_providers",
            "executionProviders",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            VariantId,
            Runtime,
            Arch,
            Format,
            Quant,
            Uri,
            Sha256,
            SizeBytes,
            MinRamGb,
            ExecutionProviders,
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
                            "variantId" | "variant_id" => Ok(GeneratedField::VariantId),
                            "runtime" => Ok(GeneratedField::Runtime),
                            "arch" => Ok(GeneratedField::Arch),
                            "format" => Ok(GeneratedField::Format),
                            "quant" => Ok(GeneratedField::Quant),
                            "uri" => Ok(GeneratedField::Uri),
                            "sha256" => Ok(GeneratedField::Sha256),
                            "sizeBytes" | "size_bytes" => Ok(GeneratedField::SizeBytes),
                            "minRamGb" | "min_ram_gb" => Ok(GeneratedField::MinRamGb),
                            "executionProviders" | "execution_providers" => Ok(GeneratedField::ExecutionProviders),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ModelVariant;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.catalog.ModelVariant")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ModelVariant, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut variant_id__ = None;
                let mut runtime__ = None;
                let mut arch__ = None;
                let mut format__ = None;
                let mut quant__ = None;
                let mut uri__ = None;
                let mut sha256__ = None;
                let mut size_bytes__ = None;
                let mut min_ram_gb__ = None;
                let mut execution_providers__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::VariantId => {
                            if variant_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("variantId"));
                            }
                            variant_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Runtime => {
                            if runtime__.is_some() {
                                return Err(serde::de::Error::duplicate_field("runtime"));
                            }
                            runtime__ = Some(map_.next_value::<Runtime>()? as i32);
                        }
                        GeneratedField::Arch => {
                            if arch__.is_some() {
                                return Err(serde::de::Error::duplicate_field("arch"));
                            }
                            arch__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Format => {
                            if format__.is_some() {
                                return Err(serde::de::Error::duplicate_field("format"));
                            }
                            format__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Quant => {
                            if quant__.is_some() {
                                return Err(serde::de::Error::duplicate_field("quant"));
                            }
                            quant__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Uri => {
                            if uri__.is_some() {
                                return Err(serde::de::Error::duplicate_field("uri"));
                            }
                            uri__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Sha256 => {
                            if sha256__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sha256"));
                            }
                            sha256__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SizeBytes => {
                            if size_bytes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sizeBytes"));
                            }
                            size_bytes__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::MinRamGb => {
                            if min_ram_gb__.is_some() {
                                return Err(serde::de::Error::duplicate_field("minRamGb"));
                            }
                            min_ram_gb__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ExecutionProviders => {
                            if execution_providers__.is_some() {
                                return Err(serde::de::Error::duplicate_field("executionProviders"));
                            }
                            execution_providers__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(ModelVariant {
                    variant_id: variant_id__.unwrap_or_default(),
                    runtime: runtime__.unwrap_or_default(),
                    arch: arch__.unwrap_or_default(),
                    format: format__.unwrap_or_default(),
                    quant: quant__.unwrap_or_default(),
                    uri: uri__.unwrap_or_default(),
                    sha256: sha256__.unwrap_or_default(),
                    size_bytes: size_bytes__.unwrap_or_default(),
                    min_ram_gb: min_ram_gb__.unwrap_or_default(),
                    execution_providers: execution_providers__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.catalog.ModelVariant", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for QualityTier {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "QUALITY_TIER_UNSPECIFIED",
            Self::Standard => "QUALITY_TIER_STANDARD",
            Self::Reduced => "QUALITY_TIER_REDUCED",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for QualityTier {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "QUALITY_TIER_UNSPECIFIED",
            "QUALITY_TIER_STANDARD",
            "QUALITY_TIER_REDUCED",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = QualityTier;

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
                    "QUALITY_TIER_UNSPECIFIED" => Ok(QualityTier::Unspecified),
                    "QUALITY_TIER_STANDARD" => Ok(QualityTier::Standard),
                    "QUALITY_TIER_REDUCED" => Ok(QualityTier::Reduced),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for Runtime {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "RUNTIME_UNSPECIFIED",
            Self::Mlx => "RUNTIME_MLX",
            Self::Onnx => "RUNTIME_ONNX",
            Self::LlamaCpp => "RUNTIME_LLAMA_CPP",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for Runtime {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "RUNTIME_UNSPECIFIED",
            "RUNTIME_MLX",
            "RUNTIME_ONNX",
            "RUNTIME_LLAMA_CPP",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Runtime;

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
                    "RUNTIME_UNSPECIFIED" => Ok(Runtime::Unspecified),
                    "RUNTIME_MLX" => Ok(Runtime::Mlx),
                    "RUNTIME_ONNX" => Ok(Runtime::Onnx),
                    "RUNTIME_LLAMA_CPP" => Ok(Runtime::LlamaCpp),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for SeedingState {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.device_id.is_empty() {
            len += 1;
        }
        if !self.models.is_empty() {
            len += 1;
        }
        if self.disk_used_bytes != 0 {
            len += 1;
        }
        if self.disk_budget_bytes != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.catalog.SeedingState", len)?;
        if !self.device_id.is_empty() {
            struct_ser.serialize_field("deviceId", &self.device_id)?;
        }
        if !self.models.is_empty() {
            struct_ser.serialize_field("models", &self.models)?;
        }
        if self.disk_used_bytes != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("diskUsedBytes", ToString::to_string(&self.disk_used_bytes).as_str())?;
        }
        if self.disk_budget_bytes != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("diskBudgetBytes", ToString::to_string(&self.disk_budget_bytes).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SeedingState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "device_id",
            "deviceId",
            "models",
            "disk_used_bytes",
            "diskUsedBytes",
            "disk_budget_bytes",
            "diskBudgetBytes",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            DeviceId,
            Models,
            DiskUsedBytes,
            DiskBudgetBytes,
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
                            "deviceId" | "device_id" => Ok(GeneratedField::DeviceId),
                            "models" => Ok(GeneratedField::Models),
                            "diskUsedBytes" | "disk_used_bytes" => Ok(GeneratedField::DiskUsedBytes),
                            "diskBudgetBytes" | "disk_budget_bytes" => Ok(GeneratedField::DiskBudgetBytes),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SeedingState;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.catalog.SeedingState")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SeedingState, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut device_id__ = None;
                let mut models__ = None;
                let mut disk_used_bytes__ = None;
                let mut disk_budget_bytes__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::DeviceId => {
                            if device_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("deviceId"));
                            }
                            device_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Models => {
                            if models__.is_some() {
                                return Err(serde::de::Error::duplicate_field("models"));
                            }
                            models__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DiskUsedBytes => {
                            if disk_used_bytes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("diskUsedBytes"));
                            }
                            disk_used_bytes__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DiskBudgetBytes => {
                            if disk_budget_bytes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("diskBudgetBytes"));
                            }
                            disk_budget_bytes__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(SeedingState {
                    device_id: device_id__.unwrap_or_default(),
                    models: models__.unwrap_or_default(),
                    disk_used_bytes: disk_used_bytes__.unwrap_or_default(),
                    disk_budget_bytes: disk_budget_bytes__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.catalog.SeedingState", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ToolSupport {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "TOOL_SUPPORT_UNSPECIFIED",
            Self::Full => "TOOL_SUPPORT_FULL",
            Self::Weak => "TOOL_SUPPORT_WEAK",
            Self::None => "TOOL_SUPPORT_NONE",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ToolSupport {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "TOOL_SUPPORT_UNSPECIFIED",
            "TOOL_SUPPORT_FULL",
            "TOOL_SUPPORT_WEAK",
            "TOOL_SUPPORT_NONE",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ToolSupport;

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
                    "TOOL_SUPPORT_UNSPECIFIED" => Ok(ToolSupport::Unspecified),
                    "TOOL_SUPPORT_FULL" => Ok(ToolSupport::Full),
                    "TOOL_SUPPORT_WEAK" => Ok(ToolSupport::Weak),
                    "TOOL_SUPPORT_NONE" => Ok(ToolSupport::None),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
