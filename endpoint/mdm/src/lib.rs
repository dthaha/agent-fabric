//! Endpoint MDM ingest: parses Intune/Jamf policy packs delivered to the
//! device and loads them as the endpoint policy ceiling. The endpoint can
//! tighten this ceiling locally but never loosen it.
