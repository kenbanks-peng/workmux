use super::*;

#[test]
fn detection_preserves_other_backends_and_nested_tmux() {
    for backend in [
        BackendType::Tmux,
        BackendType::WezTerm,
        BackendType::Kitty,
        BackendType::Zellij,
    ] {
        assert_eq!(detect_backend_with_signals(backend, false, false), backend);
        assert_eq!(detect_backend_with_signals(backend, true, true), backend);
        assert_eq!(
            detect_backend_with_signals(backend, false, true),
            if backend == BackendType::Tmux {
                BackendType::Herdr
            } else {
                backend
            }
        );
    }
}
