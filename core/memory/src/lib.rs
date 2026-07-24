//! Memory plane: SOUL loader, Honcho-class hosted source of truth adapter,
//! and the opportunistic endpoint cache. Memory is NOT leased — writes are
//! opportunistic and converge, unlike the leased context op-log.
