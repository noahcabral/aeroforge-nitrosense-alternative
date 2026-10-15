fn main() {
    if std::env::var("PROFILE").as_deref() == Ok("debug")
        && std::env::var_os("TAURI_CONFIG").is_none()
    {
        // Debug/test builds should not require release bundle artifacts to exist first.
        std::env::set_var("TAURI_CONFIG", r#"{"bundle":{"resources":null}}"#);
    }

    tauri_build::build()
}
