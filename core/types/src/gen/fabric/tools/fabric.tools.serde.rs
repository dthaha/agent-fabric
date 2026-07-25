// @generated
impl serde::Serialize for CaptureMode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "CAPTURE_MODE_UNSPECIFIED",
            Self::Som => "CAPTURE_MODE_SOM",
            Self::Vision => "CAPTURE_MODE_VISION",
            Self::Ax => "CAPTURE_MODE_AX",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for CaptureMode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "CAPTURE_MODE_UNSPECIFIED",
            "CAPTURE_MODE_SOM",
            "CAPTURE_MODE_VISION",
            "CAPTURE_MODE_AX",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = CaptureMode;

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
                    "CAPTURE_MODE_UNSPECIFIED" => Ok(CaptureMode::Unspecified),
                    "CAPTURE_MODE_SOM" => Ok(CaptureMode::Som),
                    "CAPTURE_MODE_VISION" => Ok(CaptureMode::Vision),
                    "CAPTURE_MODE_AX" => Ok(CaptureMode::Ax),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for CuaActionRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.action != 0 {
            len += 1;
        }
        if !self.element.is_empty() {
            len += 1;
        }
        if !self.coordinate.is_empty() {
            len += 1;
        }
        if !self.text.is_empty() {
            len += 1;
        }
        if !self.keys.is_empty() {
            len += 1;
        }
        if self.capture_after {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.tools.CuaActionRequest", len)?;
        if self.action != 0 {
            let v = CuaActionType::try_from(self.action)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.action)))?;
            struct_ser.serialize_field("action", &v)?;
        }
        if !self.element.is_empty() {
            struct_ser.serialize_field("element", &self.element)?;
        }
        if !self.coordinate.is_empty() {
            struct_ser.serialize_field("coordinate", &self.coordinate)?;
        }
        if !self.text.is_empty() {
            struct_ser.serialize_field("text", &self.text)?;
        }
        if !self.keys.is_empty() {
            struct_ser.serialize_field("keys", &self.keys)?;
        }
        if self.capture_after {
            struct_ser.serialize_field("captureAfter", &self.capture_after)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for CuaActionRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "action",
            "element",
            "coordinate",
            "text",
            "keys",
            "capture_after",
            "captureAfter",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Action,
            Element,
            Coordinate,
            Text,
            Keys,
            CaptureAfter,
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
                            "action" => Ok(GeneratedField::Action),
                            "element" => Ok(GeneratedField::Element),
                            "coordinate" => Ok(GeneratedField::Coordinate),
                            "text" => Ok(GeneratedField::Text),
                            "keys" => Ok(GeneratedField::Keys),
                            "captureAfter" | "capture_after" => Ok(GeneratedField::CaptureAfter),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = CuaActionRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.tools.CuaActionRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<CuaActionRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut action__ = None;
                let mut element__ = None;
                let mut coordinate__ = None;
                let mut text__ = None;
                let mut keys__ = None;
                let mut capture_after__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Action => {
                            if action__.is_some() {
                                return Err(serde::de::Error::duplicate_field("action"));
                            }
                            action__ = Some(map_.next_value::<CuaActionType>()? as i32);
                        }
                        GeneratedField::Element => {
                            if element__.is_some() {
                                return Err(serde::de::Error::duplicate_field("element"));
                            }
                            element__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Coordinate => {
                            if coordinate__.is_some() {
                                return Err(serde::de::Error::duplicate_field("coordinate"));
                            }
                            coordinate__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Text => {
                            if text__.is_some() {
                                return Err(serde::de::Error::duplicate_field("text"));
                            }
                            text__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Keys => {
                            if keys__.is_some() {
                                return Err(serde::de::Error::duplicate_field("keys"));
                            }
                            keys__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CaptureAfter => {
                            if capture_after__.is_some() {
                                return Err(serde::de::Error::duplicate_field("captureAfter"));
                            }
                            capture_after__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(CuaActionRequest {
                    action: action__.unwrap_or_default(),
                    element: element__.unwrap_or_default(),
                    coordinate: coordinate__.unwrap_or_default(),
                    text: text__.unwrap_or_default(),
                    keys: keys__.unwrap_or_default(),
                    capture_after: capture_after__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.tools.CuaActionRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for CuaActionType {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "CUA_ACTION_TYPE_UNSPECIFIED",
            Self::Click => "CUA_ACTION_TYPE_CLICK",
            Self::DoubleClick => "CUA_ACTION_TYPE_DOUBLE_CLICK",
            Self::RightClick => "CUA_ACTION_TYPE_RIGHT_CLICK",
            Self::Type => "CUA_ACTION_TYPE_TYPE",
            Self::Key => "CUA_ACTION_TYPE_KEY",
            Self::Scroll => "CUA_ACTION_TYPE_SCROLL",
            Self::Drag => "CUA_ACTION_TYPE_DRAG",
            Self::Wait => "CUA_ACTION_TYPE_WAIT",
            Self::FocusApp => "CUA_ACTION_TYPE_FOCUS_APP",
            Self::ListApps => "CUA_ACTION_TYPE_LIST_APPS",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for CuaActionType {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "CUA_ACTION_TYPE_UNSPECIFIED",
            "CUA_ACTION_TYPE_CLICK",
            "CUA_ACTION_TYPE_DOUBLE_CLICK",
            "CUA_ACTION_TYPE_RIGHT_CLICK",
            "CUA_ACTION_TYPE_TYPE",
            "CUA_ACTION_TYPE_KEY",
            "CUA_ACTION_TYPE_SCROLL",
            "CUA_ACTION_TYPE_DRAG",
            "CUA_ACTION_TYPE_WAIT",
            "CUA_ACTION_TYPE_FOCUS_APP",
            "CUA_ACTION_TYPE_LIST_APPS",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = CuaActionType;

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
                    "CUA_ACTION_TYPE_UNSPECIFIED" => Ok(CuaActionType::Unspecified),
                    "CUA_ACTION_TYPE_CLICK" => Ok(CuaActionType::Click),
                    "CUA_ACTION_TYPE_DOUBLE_CLICK" => Ok(CuaActionType::DoubleClick),
                    "CUA_ACTION_TYPE_RIGHT_CLICK" => Ok(CuaActionType::RightClick),
                    "CUA_ACTION_TYPE_TYPE" => Ok(CuaActionType::Type),
                    "CUA_ACTION_TYPE_KEY" => Ok(CuaActionType::Key),
                    "CUA_ACTION_TYPE_SCROLL" => Ok(CuaActionType::Scroll),
                    "CUA_ACTION_TYPE_DRAG" => Ok(CuaActionType::Drag),
                    "CUA_ACTION_TYPE_WAIT" => Ok(CuaActionType::Wait),
                    "CUA_ACTION_TYPE_FOCUS_APP" => Ok(CuaActionType::FocusApp),
                    "CUA_ACTION_TYPE_LIST_APPS" => Ok(CuaActionType::ListApps),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for CuaCaptureRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.app.is_empty() {
            len += 1;
        }
        if self.mode != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.tools.CuaCaptureRequest", len)?;
        if !self.app.is_empty() {
            struct_ser.serialize_field("app", &self.app)?;
        }
        if self.mode != 0 {
            let v = CaptureMode::try_from(self.mode)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.mode)))?;
            struct_ser.serialize_field("mode", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for CuaCaptureRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "app",
            "mode",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            App,
            Mode,
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
                            "app" => Ok(GeneratedField::App),
                            "mode" => Ok(GeneratedField::Mode),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = CuaCaptureRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.tools.CuaCaptureRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<CuaCaptureRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut app__ = None;
                let mut mode__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::App => {
                            if app__.is_some() {
                                return Err(serde::de::Error::duplicate_field("app"));
                            }
                            app__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Mode => {
                            if mode__.is_some() {
                                return Err(serde::de::Error::duplicate_field("mode"));
                            }
                            mode__ = Some(map_.next_value::<CaptureMode>()? as i32);
                        }
                    }
                }
                Ok(CuaCaptureRequest {
                    app: app__.unwrap_or_default(),
                    mode: mode__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.tools.CuaCaptureRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ToolDescriptor {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.tool_name.is_empty() {
            len += 1;
        }
        if !self.description.is_empty() {
            len += 1;
        }
        if self.input_schema.is_some() {
            len += 1;
        }
        if self.locality != 0 {
            len += 1;
        }
        if !self.required_permissions.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.tools.ToolDescriptor", len)?;
        if !self.tool_name.is_empty() {
            struct_ser.serialize_field("toolName", &self.tool_name)?;
        }
        if !self.description.is_empty() {
            struct_ser.serialize_field("description", &self.description)?;
        }
        if let Some(v) = self.input_schema.as_ref() {
            struct_ser.serialize_field("inputSchema", v)?;
        }
        if self.locality != 0 {
            let v = ToolLocality::try_from(self.locality)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.locality)))?;
            struct_ser.serialize_field("locality", &v)?;
        }
        if !self.required_permissions.is_empty() {
            struct_ser.serialize_field("requiredPermissions", &self.required_permissions)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ToolDescriptor {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "tool_name",
            "toolName",
            "description",
            "input_schema",
            "inputSchema",
            "locality",
            "required_permissions",
            "requiredPermissions",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ToolName,
            Description,
            InputSchema,
            Locality,
            RequiredPermissions,
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
                            "toolName" | "tool_name" => Ok(GeneratedField::ToolName),
                            "description" => Ok(GeneratedField::Description),
                            "inputSchema" | "input_schema" => Ok(GeneratedField::InputSchema),
                            "locality" => Ok(GeneratedField::Locality),
                            "requiredPermissions" | "required_permissions" => Ok(GeneratedField::RequiredPermissions),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ToolDescriptor;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.tools.ToolDescriptor")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ToolDescriptor, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut tool_name__ = None;
                let mut description__ = None;
                let mut input_schema__ = None;
                let mut locality__ = None;
                let mut required_permissions__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ToolName => {
                            if tool_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("toolName"));
                            }
                            tool_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Description => {
                            if description__.is_some() {
                                return Err(serde::de::Error::duplicate_field("description"));
                            }
                            description__ = Some(map_.next_value()?);
                        }
                        GeneratedField::InputSchema => {
                            if input_schema__.is_some() {
                                return Err(serde::de::Error::duplicate_field("inputSchema"));
                            }
                            input_schema__ = map_.next_value()?;
                        }
                        GeneratedField::Locality => {
                            if locality__.is_some() {
                                return Err(serde::de::Error::duplicate_field("locality"));
                            }
                            locality__ = Some(map_.next_value::<ToolLocality>()? as i32);
                        }
                        GeneratedField::RequiredPermissions => {
                            if required_permissions__.is_some() {
                                return Err(serde::de::Error::duplicate_field("requiredPermissions"));
                            }
                            required_permissions__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(ToolDescriptor {
                    tool_name: tool_name__.unwrap_or_default(),
                    description: description__.unwrap_or_default(),
                    input_schema: input_schema__,
                    locality: locality__.unwrap_or_default(),
                    required_permissions: required_permissions__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.tools.ToolDescriptor", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ToolLocality {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "TOOL_LOCALITY_UNSPECIFIED",
            Self::EndpointOnly => "TOOL_LOCALITY_ENDPOINT_ONLY",
            Self::ServerOk => "TOOL_LOCALITY_SERVER_OK",
            Self::Either => "TOOL_LOCALITY_EITHER",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ToolLocality {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "TOOL_LOCALITY_UNSPECIFIED",
            "TOOL_LOCALITY_ENDPOINT_ONLY",
            "TOOL_LOCALITY_SERVER_OK",
            "TOOL_LOCALITY_EITHER",
        ];

        struct GeneratedVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ToolLocality;

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
                    "TOOL_LOCALITY_UNSPECIFIED" => Ok(ToolLocality::Unspecified),
                    "TOOL_LOCALITY_ENDPOINT_ONLY" => Ok(ToolLocality::EndpointOnly),
                    "TOOL_LOCALITY_SERVER_OK" => Ok(ToolLocality::ServerOk),
                    "TOOL_LOCALITY_EITHER" => Ok(ToolLocality::Either),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ToolRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.request_id.is_empty() {
            len += 1;
        }
        if !self.session_id.is_empty() {
            len += 1;
        }
        if !self.lease_id.is_empty() {
            len += 1;
        }
        if !self.tool_name.is_empty() {
            len += 1;
        }
        if self.params.is_some() {
            len += 1;
        }
        if !self.policy_version.is_empty() {
            len += 1;
        }
        if self.requested_at.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.tools.ToolRequest", len)?;
        if !self.request_id.is_empty() {
            struct_ser.serialize_field("requestId", &self.request_id)?;
        }
        if !self.session_id.is_empty() {
            struct_ser.serialize_field("sessionId", &self.session_id)?;
        }
        if !self.lease_id.is_empty() {
            struct_ser.serialize_field("leaseId", &self.lease_id)?;
        }
        if !self.tool_name.is_empty() {
            struct_ser.serialize_field("toolName", &self.tool_name)?;
        }
        if let Some(v) = self.params.as_ref() {
            struct_ser.serialize_field("params", v)?;
        }
        if !self.policy_version.is_empty() {
            struct_ser.serialize_field("policyVersion", &self.policy_version)?;
        }
        if let Some(v) = self.requested_at.as_ref() {
            struct_ser.serialize_field("requestedAt", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ToolRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "request_id",
            "requestId",
            "session_id",
            "sessionId",
            "lease_id",
            "leaseId",
            "tool_name",
            "toolName",
            "params",
            "policy_version",
            "policyVersion",
            "requested_at",
            "requestedAt",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            RequestId,
            SessionId,
            LeaseId,
            ToolName,
            Params,
            PolicyVersion,
            RequestedAt,
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
                            "requestId" | "request_id" => Ok(GeneratedField::RequestId),
                            "sessionId" | "session_id" => Ok(GeneratedField::SessionId),
                            "leaseId" | "lease_id" => Ok(GeneratedField::LeaseId),
                            "toolName" | "tool_name" => Ok(GeneratedField::ToolName),
                            "params" => Ok(GeneratedField::Params),
                            "policyVersion" | "policy_version" => Ok(GeneratedField::PolicyVersion),
                            "requestedAt" | "requested_at" => Ok(GeneratedField::RequestedAt),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ToolRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.tools.ToolRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ToolRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut request_id__ = None;
                let mut session_id__ = None;
                let mut lease_id__ = None;
                let mut tool_name__ = None;
                let mut params__ = None;
                let mut policy_version__ = None;
                let mut requested_at__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::RequestId => {
                            if request_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("requestId"));
                            }
                            request_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SessionId => {
                            if session_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sessionId"));
                            }
                            session_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::LeaseId => {
                            if lease_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("leaseId"));
                            }
                            lease_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ToolName => {
                            if tool_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("toolName"));
                            }
                            tool_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Params => {
                            if params__.is_some() {
                                return Err(serde::de::Error::duplicate_field("params"));
                            }
                            params__ = map_.next_value()?;
                        }
                        GeneratedField::PolicyVersion => {
                            if policy_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("policyVersion"));
                            }
                            policy_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RequestedAt => {
                            if requested_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("requestedAt"));
                            }
                            requested_at__ = map_.next_value()?;
                        }
                    }
                }
                Ok(ToolRequest {
                    request_id: request_id__.unwrap_or_default(),
                    session_id: session_id__.unwrap_or_default(),
                    lease_id: lease_id__.unwrap_or_default(),
                    tool_name: tool_name__.unwrap_or_default(),
                    params: params__,
                    policy_version: policy_version__.unwrap_or_default(),
                    requested_at: requested_at__,
                })
            }
        }
        deserializer.deserialize_struct("fabric.tools.ToolRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ToolResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.request_id.is_empty() {
            len += 1;
        }
        if self.success {
            len += 1;
        }
        if self.result.is_some() {
            len += 1;
        }
        if !self.error.is_empty() {
            len += 1;
        }
        if !self.screenshot.is_empty() {
            len += 1;
        }
        if !self.screenshot_mime.is_empty() {
            len += 1;
        }
        if self.completed_at.is_some() {
            len += 1;
        }
        if !self.executed_on.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("fabric.tools.ToolResponse", len)?;
        if !self.request_id.is_empty() {
            struct_ser.serialize_field("requestId", &self.request_id)?;
        }
        if self.success {
            struct_ser.serialize_field("success", &self.success)?;
        }
        if let Some(v) = self.result.as_ref() {
            struct_ser.serialize_field("result", v)?;
        }
        if !self.error.is_empty() {
            struct_ser.serialize_field("error", &self.error)?;
        }
        if !self.screenshot.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("screenshot", pbjson::private::base64::encode(&self.screenshot).as_str())?;
        }
        if !self.screenshot_mime.is_empty() {
            struct_ser.serialize_field("screenshotMime", &self.screenshot_mime)?;
        }
        if let Some(v) = self.completed_at.as_ref() {
            struct_ser.serialize_field("completedAt", v)?;
        }
        if !self.executed_on.is_empty() {
            struct_ser.serialize_field("executedOn", &self.executed_on)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ToolResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "request_id",
            "requestId",
            "success",
            "result",
            "error",
            "screenshot",
            "screenshot_mime",
            "screenshotMime",
            "completed_at",
            "completedAt",
            "executed_on",
            "executedOn",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            RequestId,
            Success,
            Result,
            Error,
            Screenshot,
            ScreenshotMime,
            CompletedAt,
            ExecutedOn,
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
                            "requestId" | "request_id" => Ok(GeneratedField::RequestId),
                            "success" => Ok(GeneratedField::Success),
                            "result" => Ok(GeneratedField::Result),
                            "error" => Ok(GeneratedField::Error),
                            "screenshot" => Ok(GeneratedField::Screenshot),
                            "screenshotMime" | "screenshot_mime" => Ok(GeneratedField::ScreenshotMime),
                            "completedAt" | "completed_at" => Ok(GeneratedField::CompletedAt),
                            "executedOn" | "executed_on" => Ok(GeneratedField::ExecutedOn),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ToolResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct fabric.tools.ToolResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ToolResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut request_id__ = None;
                let mut success__ = None;
                let mut result__ = None;
                let mut error__ = None;
                let mut screenshot__ = None;
                let mut screenshot_mime__ = None;
                let mut completed_at__ = None;
                let mut executed_on__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::RequestId => {
                            if request_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("requestId"));
                            }
                            request_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Success => {
                            if success__.is_some() {
                                return Err(serde::de::Error::duplicate_field("success"));
                            }
                            success__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Result => {
                            if result__.is_some() {
                                return Err(serde::de::Error::duplicate_field("result"));
                            }
                            result__ = map_.next_value()?;
                        }
                        GeneratedField::Error => {
                            if error__.is_some() {
                                return Err(serde::de::Error::duplicate_field("error"));
                            }
                            error__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Screenshot => {
                            if screenshot__.is_some() {
                                return Err(serde::de::Error::duplicate_field("screenshot"));
                            }
                            screenshot__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ScreenshotMime => {
                            if screenshot_mime__.is_some() {
                                return Err(serde::de::Error::duplicate_field("screenshotMime"));
                            }
                            screenshot_mime__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CompletedAt => {
                            if completed_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("completedAt"));
                            }
                            completed_at__ = map_.next_value()?;
                        }
                        GeneratedField::ExecutedOn => {
                            if executed_on__.is_some() {
                                return Err(serde::de::Error::duplicate_field("executedOn"));
                            }
                            executed_on__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(ToolResponse {
                    request_id: request_id__.unwrap_or_default(),
                    success: success__.unwrap_or_default(),
                    result: result__,
                    error: error__.unwrap_or_default(),
                    screenshot: screenshot__.unwrap_or_default(),
                    screenshot_mime: screenshot_mime__.unwrap_or_default(),
                    completed_at: completed_at__,
                    executed_on: executed_on__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("fabric.tools.ToolResponse", FIELDS, GeneratedVisitor)
    }
}
