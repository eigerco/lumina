use std::fmt::{self, Debug};

use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Token {
    token: CancellationToken,
}

impl Token {
    pub fn new() -> Token {
        Token {
            token: CancellationToken::new(),
        }
    }

    /// Trigger the event
    pub fn trigger(&self) {
        self.token.cancel();
    }

    /// Returns a guard that will trigger the token on drop.
    pub fn trigger_drop_guard(&self) -> TokenTriggerDropGuard {
        TokenTriggerDropGuard {
            token: Some(self.token.clone()),
        }
    }

    /// Returns if event is triggered or not.
    #[allow(dead_code)]
    pub fn is_triggered(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Returns when the token is triggered. If it was triggered before,
    /// it will return immediately.
    pub async fn triggered(&self) {
        self.token.cancelled().await;
    }
}

impl Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token { .. }")
    }
}

pub struct TokenTriggerDropGuard {
    token: Option<CancellationToken>,
}

impl TokenTriggerDropGuard {
    #[allow(dead_code)]
    pub fn disarm(&mut self) {
        self.token.take();
    }
}

impl Debug for TokenTriggerDropGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TokenTriggerDropGuard { .. }")
    }
}

impl Drop for TokenTriggerDropGuard {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            token.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::{sleep, spawn};
    use crate::test_utils::async_test;
    use std::time::Duration;
    use web_time::Instant;

    #[async_test]
    async fn smoke() {
        let token = Token::new();
        assert!(!token.is_triggered());

        let now = Instant::now();

        spawn({
            let token = token.clone();
            async move {
                sleep(Duration::from_millis(10)).await;
                token.trigger();
            }
        });

        token.triggered().await;
        assert!(now.elapsed() >= Duration::from_millis(10));
        assert!(token.is_triggered());

        // This must return immediately.
        token.triggered().await;
    }

    #[async_test]
    async fn token_drop_guard() {
        let token = Token::new();
        assert!(!token.is_triggered());

        let now = Instant::now();

        spawn({
            let guard = token.trigger_drop_guard();
            async move {
                let _guard = guard;
                sleep(Duration::from_millis(10)).await;
            }
        });

        token.triggered().await;
        assert!(now.elapsed() >= Duration::from_millis(10));
        assert!(token.is_triggered());

        // This must return immediately.
        token.triggered().await;
    }

    #[async_test]
    async fn token_disarm_drop_guard() {
        let token = Token::new();
        assert!(!token.is_triggered());

        let mut guard = token.trigger_drop_guard();
        guard.disarm();
        drop(guard);

        assert!(!token.is_triggered());
    }
}
