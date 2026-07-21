/// Complete fixed security-header block for every response.
pub(crate) const SECURITY_HEADERS: &str = concat!(
    "Cache-Control: no-store\r\n",
    "Pragma: no-cache\r\n",
    "Expires: 0\r\n",
    "X-Content-Type-Options: nosniff\r\n",
    "Referrer-Policy: no-referrer\r\n",
    "X-Frame-Options: DENY\r\n",
    "Cross-Origin-Opener-Policy: same-origin\r\n",
    "Cross-Origin-Embedder-Policy: require-corp\r\n",
    "Cross-Origin-Resource-Policy: same-origin\r\n",
    "Permissions-Policy: accelerometer=(), camera=(), geolocation=(), microphone=(), payment=(), usb=()\r\n",
    "Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'none'; font-src 'none'; object-src 'none'; frame-src 'none'; worker-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; require-trusted-types-for 'script'; trusted-types 'none'\r\n",
    "Clear-Site-Data: \"cache\", \"cookies\", \"storage\"\r\n",
    "Connection: close\r\n",
);
