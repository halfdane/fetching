package web

// indexTemplate is the main page HTML template.
const indexTemplate = `{{define "index"}}<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>fetching</title>
    <style>
        :root {
            --bg: #1a1a2e;
            --surface: #16213e;
            --border: #0f3460;
            --text: #e0e0e0;
            --text-muted: #8888aa;
            --accent: #1db954;
            --accent-hover: #1ed760;
            --danger: #e74c3c;
            --warning: #f39c12;
        }
        * { box-sizing: border-box; margin: 0; padding: 0; }
        body {
            font-family: system-ui, -apple-system, sans-serif;
            background: var(--bg);
            color: var(--text);
            max-width: 800px;
            margin: 0 auto;
            padding: 2rem 1rem;
        }
        h1 { margin-bottom: 1.5rem; font-size: 1.5rem; }
        h1 span { color: var(--accent); }
        form {
            display: flex;
            gap: 0.5rem;
            margin-bottom: 2rem;
        }
        textarea {
            flex: 1;
            padding: 0.6rem 0.8rem;
            border: 1px solid var(--border);
            border-radius: 6px;
            background: var(--surface);
            color: var(--text);
            font-family: inherit;
            font-size: 0.9rem;
            resize: vertical;
            min-height: 2.5rem;
            max-height: 8rem;
        }
        textarea:focus { outline: 2px solid var(--accent); border-color: transparent; }
        button {
            padding: 0.6rem 1.2rem;
            border: none;
            border-radius: 6px;
            background: var(--accent);
            color: #000;
            font-weight: 600;
            cursor: pointer;
            white-space: nowrap;
        }
        button:hover { background: var(--accent-hover); }
        .jobs { list-style: none; }
        .job {
            padding: 0.8rem 1rem;
            border: 1px solid var(--border);
            border-radius: 6px;
            margin-bottom: 0.5rem;
            background: var(--surface);
            display: flex;
            justify-content: space-between;
            align-items: center;
            gap: 1rem;
        }
        .job-uri {
            font-family: monospace;
            font-size: 0.85rem;
            word-break: break-all;
            flex: 1;
        }
        .badge {
            padding: 0.2rem 0.6rem;
            border-radius: 4px;
            font-size: 0.75rem;
            font-weight: 600;
            text-transform: uppercase;
            white-space: nowrap;
        }
        .badge-pending  { background: var(--border); color: var(--text-muted); }
        .badge-running  { background: var(--warning); color: #000; }
        .badge-done     { background: var(--accent); color: #000; }
        .badge-failed   { background: var(--danger); color: #fff; }
        .error-msg { color: var(--danger); font-size: 0.8rem; margin-top: 0.3rem; }
        .empty { color: var(--text-muted); font-style: italic; }
        .refresh-hint {
            text-align: center;
            margin-top: 1rem;
            font-size: 0.8rem;
            color: var(--text-muted);
        }
    </style>
</head>
<body>
    <h1><span>&#9835;</span> fetching</h1>

    <form method="POST" action="/api/enqueue">
        <textarea name="uri" placeholder="Paste Spotify URI(s) or URL(s), one per line..." rows="2"></textarea>
        <button type="submit">Enqueue</button>
    </form>

    {{template "jobs" .Jobs}}

    <p class="refresh-hint">Auto-refreshes every 5 seconds</p>

    <script>
        setInterval(function() {
            fetch('/api/jobs')
                .then(r => r.json())
                .then(jobs => {
                    const container = document.getElementById('job-list');
                    if (!jobs || jobs.length === 0) {
                        container.innerHTML = '<p class="empty">No jobs in queue.</p>';
                        return;
                    }
                    let html = '';
                    jobs.forEach(j => {
                        const badge = 'badge-' + j.status;
                        html += '<li class="job">';
                        html += '<span class="job-uri">' + escapeHtml(j.spotify_uri) + '</span>';
                        html += '<span class="badge ' + badge + '">' + j.status + '</span>';
                        html += '</li>';
                        if (j.error) {
                            html += '<p class="error-msg">' + escapeHtml(j.error) + '</p>';
                        }
                    });
                    container.innerHTML = html;
                })
                .catch(() => {});
        }, 5000);

        function escapeHtml(s) {
            const div = document.createElement('div');
            div.textContent = s;
            return div.innerHTML;
        }
    </script>
</body>
</html>{{end}}`

// jobsPartial renders the job list.
const jobsPartial = `{{define "jobs"}}<ul id="job-list" class="jobs">
{{if .}}{{range .}}<li class="job">
    <span class="job-uri">{{.SpotifyURI}}</span>
    <span class="badge badge-{{.Status}}">{{.Status}}</span>
    {{if .RetryCount}}<span class="retry-count">retry {{.RetryCount}}</span>{{end}}
</li>
{{if .Error}}<p class="error-msg">{{.Error}}</p>{{end}}
{{end}}{{else}}<p class="empty">No jobs in queue.</p>{{end}}
</ul>{{end}}`
