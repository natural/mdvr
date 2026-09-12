use std::{
    io::Read,
    net::{SocketAddr, ToSocketAddrs},
};

use reqwest::{
    Url,
    blocking::{Client, Response},
    header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION},
    redirect::Policy as RedirectPolicy,
};

use super::remote_policy::{RemotePolicy, RemotePolicyError};

#[derive(Debug)]
pub(crate) enum RemoteFetchError {
    Policy(RemotePolicyError),
    InvalidUrl,
    Resolution,
    Request,
    Redirect,
    Status,
    UnsupportedMime,
    Read,
}

impl From<RemotePolicyError> for RemoteFetchError {
    fn from(error: RemotePolicyError) -> Self {
        Self::Policy(error)
    }
}

pub(crate) fn fetch_image(
    policy: &RemotePolicy,
    initial_url: &str,
) -> Result<(String, Vec<u8>), RemoteFetchError> {
    let mut request = policy.authorize(initial_url)?;
    let mut url = Url::parse(&request.url).map_err(|_| RemoteFetchError::InvalidUrl)?;
    let mut followed = 0;

    loop {
        let response = client_for(policy, &url, request.timeout)?
            .get(url.clone())
            .header(
                "Accept",
                "image/png,image/jpeg,image/webp,image/gif,image/svg+xml",
            )
            .send()
            .map_err(|_| RemoteFetchError::Request)?;

        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(RemoteFetchError::Redirect)?;
            let next = url.join(location).map_err(|_| RemoteFetchError::Redirect)?;
            request = policy.authorize_redirect(followed, next.as_str())?;
            url = next;
            followed += 1;
            continue;
        }
        if !response.status().is_success() {
            return Err(RemoteFetchError::Status);
        }
        return read_image(policy, response);
    }
}

fn client_for(
    policy: &RemotePolicy,
    url: &Url,
    timeout: std::time::Duration,
) -> Result<Client, RemoteFetchError> {
    let host = url.host_str().ok_or(RemoteFetchError::InvalidUrl)?;
    let port = url
        .port_or_known_default()
        .ok_or(RemoteFetchError::InvalidUrl)?;
    let addresses: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|_| RemoteFetchError::Resolution)?
        .collect();
    if addresses.is_empty() {
        return Err(RemoteFetchError::Resolution);
    }
    for address in &addresses {
        policy.validate_destination(address.ip())?;
    }

    Client::builder()
        .no_proxy()
        .redirect(RedirectPolicy::none())
        .timeout(timeout)
        .connect_timeout(timeout)
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|_| RemoteFetchError::Request)
}

fn read_image(
    policy: &RemotePolicy,
    response: Response,
) -> Result<(String, Vec<u8>), RemoteFetchError> {
    if let Some(bytes) = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        policy.validate_response_size(bytes)?;
    }
    let mime = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(allowed_mime)
        .ok_or(RemoteFetchError::UnsupportedMime)?;
    let max = policy.limits().max_response_bytes;
    let mut bytes = Vec::new();
    response
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RemoteFetchError::Read)?;
    policy.validate_response_size(bytes.len() as u64)?;
    Ok((mime.to_owned(), bytes))
}

fn allowed_mime(value: &str) -> Option<&'static str> {
    match value
        .split(';')
        .next()?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "image/png" => Some("image/png"),
        "image/jpeg" => Some("image/jpeg"),
        "image/webp" => Some("image/webp"),
        "image/gif" => Some("image/gif"),
        "image/svg+xml" => Some("image/svg+xml"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_images_accept_only_supported_content_types() {
        assert_eq!(allowed_mime("image/png"), Some("image/png"));
        assert_eq!(
            allowed_mime("IMAGE/SVG+XML; charset=utf-8"),
            Some("image/svg+xml")
        );
        assert_eq!(allowed_mime("text/html"), None);
        assert_eq!(allowed_mime("image/avif"), None);
    }

    #[test]
    fn resolved_private_destinations_are_rejected_before_connect() {
        let policy = RemotePolicy::new(Default::default()).unwrap();
        let url = Url::parse("http://localhost/image.png").unwrap();
        assert!(matches!(
            client_for(&policy, &url, policy.limits().timeout),
            Err(RemoteFetchError::Policy(RemotePolicyError::BlockedAddress(
                _
            )))
        ));
    }
}
