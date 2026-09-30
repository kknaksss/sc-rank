//! 동시 1건 · 간격 (`index.mjs:13-14` · `:28-39`, SPEC §2.2).
//! 블로그·플레이스가 한 잠금을 공유한다. 조회가 끝난 시점(결과가 오류여도)부터 다음 조회까지 간격을 둔다.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// `index.mjs:28` · `:36`
pub const BUSY_MESSAGE: &str = "다른 조회가 진행 중입니다. 잠시 후 다시 조회해 주세요.";
/// 블로그 간격 (`index.mjs:31`).
pub const BLOG_GAP: Duration = Duration::from_millis(700);
/// 플레이스 간격 (`index.mjs:39`).
pub const PLACE_GAP: Duration = Duration::from_millis(2000);

#[derive(Debug)]
struct State {
    busy: bool,
    next_allowed: Instant,
}

#[derive(Debug)]
pub struct Gate {
    state: Mutex<State>,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                busy: false,
                next_allowed: Instant::now(),
            }),
        }
    }
}

/// 조회 하나가 잡은 잠금. 끝나면(드롭) `busy = false`, `nextAllowed = now + gap`.
pub struct Pass<'a> {
    gate: &'a Gate,
    gap: Duration,
}

impl Gate {
    /// `if (busy || Date.now() < nextAllowed) → 429` 아니면 `busy = true`.
    pub fn enter(&self, gap: Duration) -> Result<Pass<'_>, String> {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.busy || Instant::now() < s.next_allowed {
            return Err(BUSY_MESSAGE.to_string());
        }
        s.busy = true;
        Ok(Pass { gate: self, gap })
    }
}

impl Drop for Pass<'_> {
    fn drop(&mut self) {
        let mut s = self.gate.state.lock().unwrap_or_else(|p| p.into_inner());
        s.busy = false;
        s.next_allowed = Instant::now() + self.gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 진행_중이거나_간격_미달이면_거절하고_끝난_뒤_간격이_지나면_연다() {
        let gate = Gate::default();
        let pass = gate.enter(Duration::from_millis(80)).unwrap();
        assert_eq!(
            gate.enter(Duration::ZERO).err().as_deref(),
            Some(BUSY_MESSAGE)
        );
        drop(pass);
        assert_eq!(
            gate.enter(Duration::ZERO).err().as_deref(),
            Some(BUSY_MESSAGE)
        );
        std::thread::sleep(Duration::from_millis(100));
        assert!(gate.enter(Duration::ZERO).is_ok());
    }
}
