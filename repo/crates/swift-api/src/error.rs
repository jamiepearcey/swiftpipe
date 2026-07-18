use axum::http::StatusCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiErrorCode {
    BadRequest,
    NotFound,
    PayloadTooLarge,
    ServiceUnavailable,
    Timeout,
    RateLimited,
    Unauthorized,
    Internal,
}

impl ApiErrorCode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::NotFound => "not_found",
            Self::PayloadTooLarge => "payload_too_large",
            Self::ServiceUnavailable => "service_unavailable",
            Self::Timeout => "gateway_timeout",
            Self::RateLimited => "rate_limited",
            Self::Unauthorized => "unauthorized",
            Self::Internal => "internal_error",
        }
    }

    pub(crate) const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_codes_map_to_stable_statuses_and_strings() {
        let cases = [
            (
                ApiErrorCode::BadRequest,
                StatusCode::BAD_REQUEST,
                "bad_request",
            ),
            (ApiErrorCode::NotFound, StatusCode::NOT_FOUND, "not_found"),
            (
                ApiErrorCode::PayloadTooLarge,
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
            ),
            (
                ApiErrorCode::ServiceUnavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
            ),
            (
                ApiErrorCode::Timeout,
                StatusCode::GATEWAY_TIMEOUT,
                "gateway_timeout",
            ),
            (
                ApiErrorCode::RateLimited,
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
            ),
            (
                ApiErrorCode::Unauthorized,
                StatusCode::UNAUTHORIZED,
                "unauthorized",
            ),
            (
                ApiErrorCode::Internal,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
            ),
        ];

        for (code, status, string) in cases {
            assert_eq!(code.status(), status);
            assert_eq!(code.as_str(), string);
        }
    }
}
