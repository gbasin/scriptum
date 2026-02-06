// Tauri IPC commands exposed to the frontend.

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {name}! Welcome to Scriptum.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greet_returns_welcome_message() {
        let result = greet("Alice");
        assert_eq!(result, "Hello, Alice! Welcome to Scriptum.");
    }
}
