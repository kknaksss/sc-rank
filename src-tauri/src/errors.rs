//! 오류 문구 규칙 (SPEC-001 §4).
//! PoC 가 한글 문구로 던지는 도메인 오류는 문구 그대로 행 `message` 가 되고,
//! 그 밖의 모든 오류(CDP·HTTP·IO·디코드·브라우저 기동)는 영역별 폴백 문구로 바뀐다. 원래 오류는 로그에만 남긴다.

/// `place.mjs:118`
pub const PLACE_FALLBACK: &str =
    "플레이스 조회에 실패했습니다. 브라우저 설치 또는 네이버 접근 제한을 확인해 주세요.";
/// `blog.mjs:112`
pub const BLOG_FALLBACK: &str =
    "블로그 조회 또는 이미지 처리가 실패했습니다. 잠시 후 다시 확인해 주세요.";

#[derive(Debug, thiserror::Error)]
pub enum ScrapeError {
    /// PoC 가 한글 문구로 던지는 판정 오류. 문구가 그대로 화면에 간다.
    #[error("{0}")]
    Domain(String),
    /// 그 밖의 모든 오류. 화면에는 폴백 문구만 간다.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Place,
    Blog,
}

impl ScrapeError {
    pub fn domain(message: impl Into<String>) -> Self {
        Self::Domain(message.into())
    }

    pub fn other(error: impl Into<anyhow::Error>) -> Self {
        Self::Other(error.into())
    }

    /// 행 `message` 로 쓸 문구.
    pub fn user_message(&self, area: Area) -> String {
        match self {
            Self::Domain(message) => message.clone(),
            Self::Other(_) => match area {
                Area::Place => PLACE_FALLBACK.to_string(),
                Area::Blog => BLOG_FALLBACK.to_string(),
            },
        }
    }
}

pub type Result<T> = std::result::Result<T, ScrapeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_passes_through_and_other_falls_back() {
        let domain = ScrapeError::domain("플레이스 목록을 읽지 못했습니다.");
        assert_eq!(
            domain.user_message(Area::Place),
            "플레이스 목록을 읽지 못했습니다."
        );
        assert_eq!(
            domain.user_message(Area::Blog),
            "플레이스 목록을 읽지 못했습니다."
        );
        let other = ScrapeError::other(anyhow::anyhow!("net::ERR_CONNECTION_RESET"));
        assert_eq!(other.user_message(Area::Place), PLACE_FALLBACK);
        assert_eq!(other.user_message(Area::Blog), BLOG_FALLBACK);
    }
}
