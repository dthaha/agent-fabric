//! Server-side agent loop: runs turns for sessions whose lease is server-side
//! (long-horizon, background, or weak-endpoint cases) and calls endpoint
//! tools over the authenticated bridge. Leased with the context plane.
