//! Hosted agent loop server: runs turns for sessions whose lease is hosted
//! (long-horizon, background, or weak-endpoint cases) and calls endpoint
//! tools over the authenticated bridge. Leased with the context plane.
