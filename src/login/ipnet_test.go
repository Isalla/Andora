package main

import (
	"strings"
	"testing"
)

func TestNormalizeURLHost(t *testing.T) {
	cases := []struct {
		in   string
		want string
		err  bool
	}{
		{"http://127.0.0.1:8080/x", "http://127.0.0.1:8080/x", false},
		{"ws://127.0.0.1:3001/ws", "ws://127.0.0.1:3001/ws", false},
		{"http://myhost.example:8080/x", "http://myhost.example:8080/x", false},
		{"http://[2001:db8::1]:8080/x", "http://[2001:db8::1]:8080/x", false},
		{"ws://[::1]:3001/ws", "ws://[::1]:3001/ws", false},
		{"http://2001:db8::1:8080/x", "http://[2001:db8::1]:8080/x", false},
		{"ws://::1:3001/ws", "ws://[::1]:3001/ws", false},
		{"ws://[::1]/ws", "ws://[::1]/ws", false},
		{"http://[2001:db8::1]", "http://[2001:db8::1]", false},
		{"http://::1/", "http://[::1]/", false},
		{"no-scheme-at-all", "", true},
		{"http://", "", true},
	}
	for _, c := range cases {
		got, err := normalizeURLHost(c.in)
		if c.err {
			if err == nil {
				t.Errorf("normalizeURLHost(%q): expected error, got %q", c.in, got)
			}
			continue
		}
		if err != nil {
			t.Errorf("normalizeURLHost(%q): %v", c.in, err)
			continue
		}
		if got != c.want {
			t.Errorf("normalizeURLHost(%q) = %q, want %q", c.in, got, c.want)
		}
	}
}

func TestBracketURLHostNoDoubleBracket(t *testing.T) {
	// idempotent: an already bracketed URL must not gain a second pair.
	in := "ws://[2001:db8::1]:3001/ws"
	for i := 0; i < 2; i++ {
		out, err := normalizeURLHost(in)
		if err != nil {
			t.Fatal(err)
		}
		if strings.Count(out, "[") != 1 || strings.Count(out, "]") != 1 {
			t.Fatalf("double bracketing: %q -> %q", in, out)
		}
		in = out
	}
}

func TestLoginListenHostsDual(t *testing.T) {
	plans, err := listenHosts("dual", 8081)
	if err != nil {
		t.Fatal(err)
	}
	if len(plans) != 2 {
		t.Fatalf("want 2 plans, got %d", len(plans))
	}
}
