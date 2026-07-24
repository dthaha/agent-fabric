//! Model plane: the unified logical-model catalog, endpoint seeding within
//! disk budgets, and the runtime router. OS-native backends (MLX, ONNX,
//! llama.cpp) live behind traits — the router never imports them directly.
