package main

import (
	"crypto/tls"
	"fmt"
	"net"
	"strconv"
	"strings"
)

// listenPlan is one concrete listener to open when the service starts.
type listenPlan struct {
	network string // "tcp", "tcp4" or "tcp6"
	addr    string // host:port, IPv6 hosts are bracketed
}

// listenHosts converts the configured bind host into the concrete
// listener plans. Supported values:
//
//	"" | "auto"   all interfaces, OS/platform default (dual stack where
//	              supported) — the previous behavior.
//	"ipv4" | "4"  IPv4 wildcard (0.0.0.0), single listener.
//	"ipv6" | "6"  IPv6 wildcard ([::]), single listener, IPv6-only.
//	"dual"|"both" explicit IPv4 + IPv6 wildcard (two listeners, never
//	              relies on IPv4-mapped acceptance).
//	IP literal    one listener for that single address family.
//	hostname      resolved at startup; one listener per address family
//	              the name resolves to (A + AAAA records yield two).
//
// An IPv6 literal may be written with or without surrounding brackets.
func listenHosts(host string, port int) ([]listenPlan, error) {
	h := strings.ToLower(strings.TrimSpace(host))
	switch h {
	case "", "auto":
		return []listenPlan{{network: "tcp", addr: net.JoinHostPort("", strconv.Itoa(port))}}, nil
	case "ipv4", "4":
		return []listenPlan{{network: "tcp4", addr: net.JoinHostPort("0.0.0.0", strconv.Itoa(port))}}, nil
	case "ipv6", "6":
		return []listenPlan{{network: "tcp6", addr: net.JoinHostPort("::", strconv.Itoa(port))}}, nil
	case "dual", "both":
		return []listenPlan{
			{network: "tcp4", addr: net.JoinHostPort("0.0.0.0", strconv.Itoa(port))},
			{network: "tcp6", addr: net.JoinHostPort("::", strconv.Itoa(port))},
		}, nil
	}

	if ip := parseIPLiteral(host); ip != nil {
		if ip.To4() != nil {
			return []listenPlan{{network: "tcp4", addr: net.JoinHostPort(ip.String(), strconv.Itoa(port))}}, nil
		}
		return []listenPlan{{network: "tcp6", addr: net.JoinHostPort(ip.String(), strconv.Itoa(port))}}, nil
	}

	// Hostname: one listener per address family the name resolves to.
	addrs, err := net.LookupIP(host)
	if err != nil {
		return nil, fmt.Errorf("resolve bind host %q: %w", host, err)
	}
	var plans []listenPlan
	var haveV4, haveV6 bool
	for _, ip := range addrs {
		if ip.To4() != nil {
			if haveV4 {
				continue
			}
			plans = append(plans, listenPlan{network: "tcp4", addr: net.JoinHostPort(ip.String(), strconv.Itoa(port))})
			haveV4 = true
		} else {
			if haveV6 {
				continue
			}
			plans = append(plans, listenPlan{network: "tcp6", addr: net.JoinHostPort(ip.String(), strconv.Itoa(port))})
			haveV6 = true
		}
	}
	if len(plans) == 0 {
		return nil, fmt.Errorf("bind host %q resolved to no usable address", host)
	}
	return plans, nil
}

// parseIPLiteral parses host as an IP literal, tolerating optional
// surrounding brackets (e.g. "[2001:db8::1]"). Returns nil when host
// is no IP literal.
func parseIPLiteral(host string) net.IP {
	h := strings.TrimSpace(host)
	h = strings.TrimPrefix(h, "[")
	h = strings.TrimSuffix(h, "]")
	return net.ParseIP(h)
}

// openListeners opens all listener plans, wrapping each with TLS when
// tlsCfg is non-nil. On error every already opened listener is closed.
func openListeners(plans []listenPlan, tlsCfg *tls.Config) ([]net.Listener, error) {
	lns := make([]net.Listener, 0, len(plans))
	for _, p := range plans {
		ln, err := net.Listen(p.network, p.addr)
		if err != nil {
			for _, l := range lns {
				l.Close()
			}
			return nil, fmt.Errorf("listen %s %s: %w", p.network, p.addr, err)
		}
		if tlsCfg != nil {
			ln = tls.NewListener(ln, tlsCfg)
		}
		lns = append(lns, ln)
	}
	return lns, nil
}
