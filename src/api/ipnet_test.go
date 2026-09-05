package main

import (
	"net"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

func TestParseIPLiteral(t *testing.T) {
	cases := []struct {
		in   string
		want bool
	}{
		{"0.0.0.0", true},
		{"127.0.0.1", true},
		{"192.168.1.42", true},
		{"::", true},
		{"::1", true},
		{"[::1]", true},
		{"2001:db8::1", true},
		{"[2001:db8::1]", true},
		{"fe80::1", true},
		{"localhost", false},
		{"myhost.example", false},
		{"", false},
		{"dual", false},
	}
	for _, c := range cases {
		got := parseIPLiteral(c.in)
		if (got != nil) != c.want {
			t.Errorf("parseIPLiteral(%q) = %v, want literal=%v", c.in, got, c.want)
		}
	}
}

func TestListenHosts(t *testing.T) {
	port := 12345
	cases := []struct {
		name   string
		host   string
		plans  int
		v4     bool
		v6     bool
		single string // expected network of the single plan
		errSub string
	}{
		{name: "empty", host: "", plans: 1, single: "tcp"},
		{name: "auto", host: "auto", plans: 1, single: "tcp"},

		{name: "ipv4 keyword", host: "ipv4", plans: 1, v4: true, single: "tcp4"},
		{name: "4 keyword", host: "4", plans: 1, v4: true, single: "tcp4"},
		{name: "ipv6 keyword", host: "ipv6", plans: 1, v6: true, single: "tcp6"},
		{name: "6 keyword", host: "6", plans: 1, v6: true, single: "tcp6"},
		{name: "dual keyword", host: "dual", plans: 2, v4: true, v6: true},
		{name: "both keyword", host: "both", plans: 2, v4: true, v6: true},

		{name: "v4 literal", host: "127.0.0.1", plans: 1, v4: true, single: "tcp4"},
		{name: "v4 wildcard", host: "0.0.0.0", plans: 1, v4: true, single: "tcp4"},
		{name: "v6 literal", host: "::1", plans: 1, v6: true, single: "tcp6"},
		{name: "v6 literal bracketed", host: "[::1]", plans: 1, v6: true, single: "tcp6"},
		{name: "v6 wildcard", host: "::", plans: 1, v6: true, single: "tcp6"},
		{name: "v6 wildcard bracketed", host: "[::]", plans: 1, v6: true, single: "tcp6"},
		{name: "v6 global", host: "2001:db8::1", plans: 1, v6: true, single: "tcp6"},

		{name: "bad hostname", host: "no-such-host.invalid", errSub: "resolve"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			plans, err := listenHosts(c.host, port)
			if c.errSub != "" {
				if err == nil || !strings.Contains(err.Error(), c.errSub) {
					t.Fatalf("listenHosts(%q) err = %v, want containing %q", c.host, err, c.errSub)
				}
				return
			}
			if err != nil {
				t.Fatalf("listenHosts(%q): %v", c.host, err)
			}
			if len(plans) != c.plans {
				t.Fatalf("listenHosts(%q) = %d plans, want %d", c.host, len(plans), c.plans)
			}
			if c.single != "" {
				if len(plans) != 1 || plans[0].network != c.single {
					t.Fatalf("listenHosts(%q) = %+v, want single network %q", c.host, plans, c.single)
				}
			}
			for _, p := range plans {
				host, _, err := net.SplitHostPort(p.addr)
				if err != nil {
					t.Fatalf("plan addr %q: %v", p.addr, err)
				}
				isV4 := p.network == "tcp4"
				if p.network == "tcp" {
					continue
				}
				ip := net.ParseIP(strings.Trim(host, "[]"))
				if ip == nil {
					t.Fatalf("plan addr %q has non-IP host", p.addr)
				}
				if isV4 != (ip.To4() != nil) {
					t.Fatalf("network %s but host %s", p.network, host)
				}
			}
			_ = c.v4
			_ = c.v6
		})
	}
}

func TestOpenListenersDualStack(t *testing.T) {
	ln0, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	port := ln0.Addr().(*net.TCPAddr).Port
	ln0.Close()

	plans, err := listenHosts("dual", port)
	if err != nil {
		t.Fatal(err)
	}
	lns, err := openListeners(plans, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		for _, l := range lns {
			l.Close()
		}
	}()
	if len(lns) != 2 {
		t.Fatalf("want 2 listeners, got %d", len(lns))
	}
	var v4ok, v6ok bool
	for _, ln := range lns {
		addr := ln.Addr().(*net.TCPAddr)
		if addr.IP.To4() != nil {
			if addr.IP.String() != "0.0.0.0" {
				t.Fatalf("v4 listener bound to %s, want 0.0.0.0", addr.IP)
			}
			v4ok = true
		} else {
			if addr.IP.String() != "::" {
				t.Fatalf("v6 listener bound to %s, want ::", addr.IP)
			}
			v6ok = true
		}
	}
	if !v4ok || !v6ok {
		t.Fatalf("dual listener missing family: v4=%v v6=%v", v4ok, v6ok)
	}

	conn, err := net.Dial("tcp", "127.0.0.1:"+strconv.Itoa(port))
	if err != nil {
		t.Fatalf("v4 dial to dual listeners: %v", err)
	}
	conn.Close()
}

func TestClientIP(t *testing.T) {
	trusted, err := parseTrustedProxies("1.2.3.4,10.0.0.0/8,2001:db8::/32")
	if err != nil {
		t.Fatal(err)
	}
	fakeReq := func(remote, xff, xrip string) *http.Request {
		r := httptest.NewRequest("POST", "/x", nil)
		r.RemoteAddr = remote
		if xff != "" {
			r.Header.Set("X-Forwarded-For", xff)
		}
		if xrip != "" {
			r.Header.Set("X-Real-IP", xrip)
		}
		return r
	}
	cases := []struct {
		name   string
		remote string
		xff    string
		xrip   string
		trust  *trustedProxySet
		want   string
	}{
		{"direct only", "9.9.9.9:1234", "", "", trusted, "9.9.9.9"},
		{"direct ipv6", "[2001:db8::10]:1234", "", "", trusted, "2001:db8::10"},
		{"untrusted proxy ignores xff", "1.1.1.1:1234", "6.6.6.6", "", trusted, "1.1.1.1"},
		{"trusted proxy xff", "1.2.3.4:1234", "8.8.8.8, 1.2.3.4", "", trusted, "8.8.8.8"},
		{"trusted proxy chain skip", "1.2.3.4:1234", "8.8.8.8, 10.0.0.5, 10.0.0.6", "", trusted, "8.8.8.8"},
		{"trusted cidr xff", "10.0.0.7:1234", "7.7.7.7", "", trusted, "7.7.7.7"},
		{"trusted ipv6 xff", "[2001:db8::5]:1234", "[2606:4700::55]", "", trusted, "2606:4700::55"},
		{"x-real fallback", "1.2.3.4:1234", "", "5.5.5.5", trusted, "5.5.5.5"},
		{"no config ignores xff", "1.2.3.4:1234", "8.8.8.8", "", nil, "1.2.3.4"},
		{"trusted all hops falls back", "1.2.3.4:1234", "10.0.0.1, 10.0.0.2", "", trusted, "1.2.3.4"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := clientIP(fakeReq(c.remote, c.xff, c.xrip), c.trust); got != c.want {
				t.Fatalf("clientIP = %q, want %q", got, c.want)
			}
		})
	}
}

func TestParseTrustedProxies(t *testing.T) {
	if _, err := parseTrustedProxies("1.2.3.4, not-an-ip, 10.0.0.0/8"); err == nil {
		t.Fatal("expected error for invalid entry")
	}
	set, err := parseTrustedProxies("2001:db8::1,10.0.0.0/8,[::1]")
	if err != nil {
		t.Fatal(err)
	}
	for _, ip := range []string{"2001:db8::1", "10.1.2.3", "::1"} {
		if !set.contains(net.ParseIP(ip)) {
			t.Errorf("expected %s to be trusted", ip)
		}
	}
	for _, ip := range []string{"8.8.8.8", "2001:db9::9", "::2"} {
		if set.contains(net.ParseIP(ip)) {
			t.Errorf("did not expect %s to be trusted", ip)
		}
	}
	if set.contains(nil) {
		t.Error("nil IP must not be trusted")
	}
}

func TestOpenAuthDSPrefix(t *testing.T) {
	dsn4 := mysqlDSN(AuthDBConfig{Host: "127.0.0.1", Port: 3306})
	if !strings.Contains(dsn4, "tcp(127.0.0.1:3306)/") {
		t.Errorf("v4 dsn: %s", dsn4)
	}
	dsn6 := mysqlDSN(AuthDBConfig{Host: "2001:db8::1", Port: 3306})
	if !strings.Contains(dsn6, "tcp([2001:db8::1]:3306)/") {
		t.Errorf("v6 dsn: %s", dsn6)
	}
}
