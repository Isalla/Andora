package main

import (
	"fmt"
	"net"
	"net/url"
	"strings"
)

// parseURL is a tiny indirection so normalizeURLHost can validate the
// result without importing net/url at every call site.
func parseURL(raw string) (*url.URL, error) { return url.Parse(raw) }

// normalizeURLHost brackets an unbracketed IPv6 literal in the host
// part of a URL so the value is usable in requests, e.g.
// "http://2001:db8::1:8080/x" becomes "http://[2001:db8::1]:8080/x".
//
// The interpretation of an unbracketed IPv6-like authority is
// host:port FIRST (the last colon separates a numeric port). A bare
// IPv6 address has no unambiguous "no port" reading when its final
// hextet is numeric, therefore bare IPv6 addresses must be written
// bracketed (e.g. "ws://[::1]/ws"); the loader still accepts the
// whole-authority-is-an-address form and wraps it when the trailing
// part is not a plausible decimal port.
//
// IPv4 hosts, hostnames and already bracketed IPv6 authorities are
// returned unchanged. The result is validated with net/url.Parse so a
// malformed value fails fast at config load.
func normalizeURLHost(raw string) (string, error) {
	if raw == "" {
		return "", nil
	}
	out := bracketURLHost(raw)
	u, err := parseURL(out)
	if err != nil {
		return "", fmt.Errorf("invalid URL %q: %w", raw, err)
	}
	if u.Scheme == "" || u.Host == "" {
		return "", fmt.Errorf("invalid URL %q: scheme and host are required", raw)
	}
	return out, nil
}

// bracketURLHost performs the bracketing step of normalizeURLHost.
func bracketURLHost(raw string) string {
	schemeEnd := strings.Index(raw, "://")
	if schemeEnd < 0 {
		return raw
	}
	rest := raw[schemeEnd+3:]
	authEnd := len(rest)
	for i := 0; i < len(rest); i++ {
		if rest[i] == '/' || rest[i] == '?' || rest[i] == '#' {
			authEnd = i
			break
		}
	}
	auth := rest[:authEnd]
	if auth == "" || strings.HasPrefix(auth, "[") || !strings.Contains(auth, ":") {
		return raw
	}

	// host:port form: last colon separates a plausible decimal port.
	if li := strings.LastIndex(auth, ":"); li > 0 {
		hostPart, portPart := auth[:li], auth[li+1:]
		if n, ok := decimalPort(portPart); ok && n <= 65535 {
			if ip := net.ParseIP(strings.Trim(hostPart, "[]")); ip != nil && ip.To4() == nil {
				return raw[:schemeEnd+3] + "[" + hostPart + "]:" + portPart + rest[authEnd:]
			}
		}
	}

	// Bare IPv6 address (no port).
	if ip := net.ParseIP(auth); ip != nil && ip.To4() == nil {
		return raw[:schemeEnd+3] + "[" + auth + "]" + rest[authEnd:]
	}
	return raw
}

// decimalPort reports whether s is a non-negative decimal integer.
func decimalPort(s string) (int, bool) {
	if s == "" {
		return 0, false
	}
	for _, r := range s {
		if r < '0' || r > '9' {
			return 0, false
		}
	}
	n := 0
	for _, r := range s {
		n = n*10 + int(r-'0')
	}
	return n, true
}
