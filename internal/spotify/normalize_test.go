package spotify

import "testing"

func TestNormalizeURI(t *testing.T) {
	cases := []struct {
		in   string
		want string
	}{
		{"spotify:album:7u20BJTgQrowjyaTEgE46p", "spotify:album:7u20BJTgQrowjyaTEgE46p"},
		{"https://open.spotify.com/album/7u20BJTgQrowjyaTEgE46p?si=UXCM7hLZQcCHD7kXSrLT-g", "https://open.spotify.com/album/7u20BJTgQrowjyaTEgE46p"},
		{"https://open.spotify.com/album/7FwAtuhhWivxvK4aPgyyUD?si=a_NcT3pYQo2yLHReFORr8w", "https://open.spotify.com/album/7FwAtuhhWivxvK4aPgyyUD"},
		{"https://open.spotify.com/album/7FwAtuhhWivxvK4aPgyyUD?foo=bar&si=abc", "https://open.spotify.com/album/7FwAtuhhWivxvK4aPgyyUD"},
		{"https://open.spotify.com/album/7FwAtuhhWivxvK4aPgyyUD#fragment?si=abc", "https://open.spotify.com/album/7FwAtuhhWivxvK4aPgyyUD"},
		{"https://open.spotify.com/album/7FwAtuhhWivxvK4aPgyyUD", "https://open.spotify.com/album/7FwAtuhhWivxvK4aPgyyUD"},
		{"http://open.spotify.com/album/7FwAtuhhWivxvK4aPgyyUD?si=abc", "http://open.spotify.com/album/7FwAtuhhWivxvK4aPgyyUD"},
		{"not-a-url", "not-a-url"},
	}
	for _, c := range cases {
		got := NormalizeURI(c.in)
		if got != c.want {
			t.Errorf("NormalizeURI(%q) = %q, want %q", c.in, got, c.want)
		}
	}
}
