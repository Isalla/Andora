package main

import (
	"fmt"
	"net"
	"net/http"
	"strings"
)

// trustedProxySet is the set of explicitly configured proxies whose
// X-Forwarded-For / X-Real-IP headers are believed. Fail-closed:
// without configuration no forwarding header is ever honored.
type trustedProxySet struct {
	ips  []net.IP
	cidr []*net.IPNet
}

// parseTrustedProxies parses the comma-separated TRUSTED_PROXIES value
// into a set of IP literals and CIDR networks.
func parseTrustedProxies(v string) (*trustedProxySet, error) {
	set := &trustedProxySet{}
	if strings.TrimSpace(v) == "" {
		return set, nil
	}
	for _, part := range strings.Split(v, ",") {
		part = strings.TrimSpace(part)
		if part == "" {
			continue
		}
		part = strings.TrimPrefix(part, "[")
		part = strings.TrimSuffix(part, "]")
		if ip := net.ParseIP(part); ip != nil {
			set.ips = append(set.ips, ip)
			continue
		}
		if _, ipNet, err := net.ParseCIDR(part); err == nil {
			set.cidr = append(set.cidr, ipNet)
			continue
		}
		return nil, fmt.Errorf("invalid TRUSTED_PROXIES entry %q", part)
	}
	return set, nil
}

// contains reports whether ip is an explicitly trusted proxy.
func (s *trustedProxySet) contains(ip net.IP) bool {
	if s == nil {
		return false
	}
	for _, t := range s.ips {
		if t.Equal(ip) {
			return true
		}
	}
	for _, n := range s.cidr {
		if n.Contains(ip) {
			return true
		}
	}
	return false
}

// clientIP resolves the client IP of a request. The direct peer is
// always used. X-Forwarded-For is only honored when the direct peer is
// in the trusted set: walking the header right to left, every hop that
// is itself a trusted proxy is skipped and the first untrusted entry
// (the client) wins. X-Real-IP is used as a fallback when the direct
// peer is trusted but no X-Forwarded-For is present.
func clientIP(r *http.Request, trusted *trustedProxySet) string {
	direct, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		direct = r.RemoteAddr
	}
	direct = strings.Trim(direct, "[]")
	if direct == "" {
		return ""
	}
	dirIP := net.ParseIP(direct)
	if dirIP == nil || !trusted.contains(dirIP) {
		return direct
	}
	if fwd := r.Header.Get("X-Forwarded-For"); fwd != "" {
		hops := strings.Split(fwd, ",")
		for i := len(hops) - 1; i >= 0; i-- {
			hop := strings.TrimSpace(strings.Trim(hops[i], "[]"))
			if hop == "" {
				continue
			}
			ip := net.ParseIP(hop)
			if ip != nil && trusted.contains(ip) {
				continue
			}
			return hop
		}
	}
	if rip := strings.TrimSpace(strings.Trim(r.Header.Get("X-Real-IP"), "[]")); rip != "" {
		return rip
	}
	return direct
}
