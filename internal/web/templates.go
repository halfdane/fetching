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
			--blue: #4ea1ff;
        }
        * { box-sizing: border-box; margin: 0; padding: 0; }
        body {
            font-family: system-ui, -apple-system, sans-serif;
            background: var(--bg);
            color: var(--text);
            max-width: 980px;
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
        input[type="text"] {
            flex: 1;
            padding: 0.6rem 0.8rem;
            border: 1px solid var(--border);
            border-radius: 6px;
            background: var(--surface);
            color: var(--text);
            font-family: inherit;
            font-size: 0.9rem;
        }
        input[type="text"]:focus { outline: 2px solid var(--accent); border-color: transparent; }
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
        .jobs { list-style: none; display: grid; gap: 0.75rem; }
        .collection {
            padding: 0.9rem 1rem;
            border: 1px solid var(--border);
            border-radius: 6px;
            background: var(--surface);
        }
        .collection-header {
            display: flex;
            align-items: flex-start;
            gap: 0.75rem;
        }
        .cover {
            width: 56px;
            height: 56px;
            border-radius: 4px;
            object-fit: cover;
            background: #243058;
            flex: 0 0 56px;
        }
        .cover-placeholder {
            display: grid;
            place-items: center;
            font-size: 0.75rem;
            color: var(--text-muted);
            border: 1px dashed var(--border);
        }
        .collection-main { flex: 1; min-width: 0; }
        .collection-title {
            font-size: 0.98rem;
            font-weight: 700;
            margin-bottom: 0.2rem;
            word-break: break-word;
        }
        .collection-meta {
            color: var(--text-muted);
            font-size: 0.8rem;
            margin-bottom: 0.45rem;
        }
        .progress {
            width: 100%;
            height: 7px;
            background: #0f1c38;
            border-radius: 999px;
            overflow: hidden;
        }
        .progress-fill {
            height: 100%;
            background: var(--accent);
        }
        .collection-actions {
            display: flex;
            flex-direction: column;
            gap: 0.4rem;
            align-items: flex-end;
            margin-left: 0.5rem;
        }
        .toggle {
            background: transparent;
            color: var(--text-muted);
            border: 1px solid var(--border);
            padding: 0.2rem 0.5rem;
            border-radius: 4px;
            font-size: 0.75rem;
        }
        .toggle:hover { background: #1b2b4f; }
        .retry-btn {
            background: var(--warning);
            color: #111;
            font-size: 0.75rem;
            padding: 0.3rem 0.6rem;
        }
        .tracks {
            list-style: none;
            margin-top: 0.7rem;
            border-top: 1px solid var(--border);
            padding-top: 0.55rem;
        }
        .track {
            display: flex;
            justify-content: space-between;
            gap: 0.8rem;
            padding: 0.3rem 0;
            font-size: 0.85rem;
        }
        .track-name { min-width: 0; flex: 1; }
        .track-state {
            font-size: 0.78rem;
            white-space: nowrap;
            color: var(--blue);
        }
        .state-red { color: var(--danger); }
        .state-green { color: var(--accent); }
        .state-blue { color: var(--blue); }
        .duration { color: var(--text-muted); font-size: 0.75rem; margin-left: 0.4rem; }
        .error-msg { color: var(--danger); font-size: 0.8rem; margin-top: 0.45rem; }
        .empty { color: var(--text-muted); font-style: italic; }
        .refresh-hint {
            text-align: center;
            margin-top: 1rem;
            font-size: 0.8rem;
            color: var(--text-muted);
        }
        .hidden { display: none; }
    </style>
</head>
<body>
    <h1><span>&#9835;</span> fetching</h1>

    <form id="enqueue-form" method="POST" action="/api/jobs">
        <input id="uri-input" type="text" name="uri" placeholder="Paste one Spotify URI or URL and press Enter" autocomplete="off" required>
        <button type="submit">Add</button>
    </form>

    {{template "jobs" .Collections}}

    <p class="refresh-hint">Live updates enabled</p>

    <script>
        const expanded = {};
        let latestCollections = [];
        const list = document.getElementById('job-list');
        const form = document.getElementById('enqueue-form');
        const uriInput = document.getElementById('uri-input');

        form.addEventListener('submit', function(e) {
            e.preventDefault();
            const uri = uriInput.value.trim();
            if (!uri) return;

            fetch('/api/jobs', {
                method: 'POST',
                headers: { 'Accept': 'application/json', 'Content-Type': 'application/x-www-form-urlencoded' },
                body: 'uri=' + encodeURIComponent(uri)
            }).then(resp => {
                if (!resp.ok) {
                    return resp.text().then(msg => Promise.reject(msg || 'submit failed'));
                }
                uriInput.value = '';
            }).catch(err => {
                alert('Failed to submit: ' + err);
            });
        });

        function renderCollections(collections) {
            latestCollections = collections || [];
            if (!collections || collections.length === 0) {
                list.innerHTML = '<p class="empty">No jobs yet.</p>';
                return;
            }
            let html = '';
            collections.forEach(c => {
                const total = c.totalTracks || 0;
                const done = c.doneTracks || 0;
                const pct = total > 0 ? Math.round((done / total) * 100) : 0;
                const isExpanded = !!expanded[c.jobId];
                const canRetry = c.terminal && c.failedTracks > 0;

                html += '<li class="collection">';
                html += '<div class="collection-header">';
                if (c.coverUrl && !c.placeholderCover) {
                    html += '<img class="cover" src="' + escapeHtml(c.coverUrl) + '" alt="cover">';
                } else {
                    const placeholderLabel = (c.kind === 'playlist') ? 'Playlist' : 'Cover';
                    html += '<div class="cover cover-placeholder">' + placeholderLabel + '</div>';
                }
                html += '<div class="collection-main">';
                html += '<div class="collection-title">' + escapeHtml(c.title || c.sourceUri || 'Loading…') + '</div>';
                html += '<div class="collection-meta">' + escapeHtml(c.kind || 'collection') + ' · ' + done + '/' + total + ' done';
                if (c.failedTracks > 0) html += ' · ' + c.failedTracks + ' failed';
                html += '</div>';
                html += '<div class="progress"><div class="progress-fill" style="width:' + pct + '%"></div></div>';
                html += '</div>';
                html += '<div class="collection-actions">';
                html += '<button class="toggle" type="button" data-toggle="' + c.jobId + '">' + (isExpanded ? 'Collapse' : 'Expand') + '</button>';
                if (canRetry) {
                    html += '<button class="retry-btn" type="button" data-retry="' + c.jobId + '" data-uri="' + escapeHtml(c.sourceUri) + '">Retry</button>';
                }
                html += '</div>';
                html += '</div>';

                html += '<ul class="tracks ' + (isExpanded ? '' : 'hidden') + '" id="tracks-' + c.jobId + '">';
                (c.tracks || []).forEach(t => {
                    const stateClass = statusClass(t.status);
                    const stateText = statusText(t);
                    html += '<li class="track">';
                    html += '<div class="track-name">' + escapeHtml(t.title || 'Loading metadata…');
                    if (t.durationSec > 0) {
                        html += '<span class="duration">' + formatDuration(t.durationSec) + '</span>';
                    }
                    html += '</div>';
                    html += '<div class="track-state ' + stateClass + '">' + escapeHtml(stateText) + '</div>';
                    html += '</li>';
                });
                html += '</ul>';
                html += '</li>';
            });

            list.innerHTML = html;

            document.querySelectorAll('[data-toggle]').forEach(btn => {
                btn.onclick = function() {
                    const id = this.getAttribute('data-toggle');
                    expanded[id] = !expanded[id];
                    renderCollections(latestCollections);
                };
            });

            document.querySelectorAll('[data-retry]').forEach(btn => {
                btn.onclick = function() {
                    const uri = this.getAttribute('data-uri');
                    fetch('/api/jobs/retry', {
                        method: 'POST',
                        headers: { 'Accept': 'application/json', 'Content-Type': 'application/x-www-form-urlencoded' },
                        body: 'uri=' + encodeURIComponent(uri)
                    });
                };
            });
        }

        function statusClass(status) {
            if (status === 'failed' || status === 'retry_waiting') return 'state-red';
            if (status === 'done' || status === 'already_present') return 'state-green';
            return 'state-blue';
        }

        function statusText(track) {
            switch (track.status) {
                case 'queued': return 'Queued';
                case 'resolving_metadata': return 'Fetching metadata';
                case 'downloading_audio': return 'Fetching audio data';
                case 'downloading_cover': return 'Fetching cover art';
                case 'tagging': return 'Tagging';
                case 'retry_waiting': return 'Retry ' + (track.retryAttempt || 0) + '/' + (track.retryMax || 0) + ' in ' + (track.retryInSec || 0) + 's';
                case 'already_present': return 'Already present';
                case 'done': return 'Done';
                case 'failed': return 'Failed';
                default: return 'Working';
            }
        }

        function formatDuration(totalSec) {
            const m = Math.floor(totalSec / 60);
            const s = totalSec % 60;
            return m + ':' + String(s).padStart(2, '0');
        }

        function connectStream() {
            const es = new EventSource('/api/stream');
            es.addEventListener('snapshot', function(ev) {
                try {
                    const payload = JSON.parse(ev.data);
                    renderCollections(payload.collections || []);
                } catch (_) {}
            });
            es.onerror = function() {
                es.close();
                setTimeout(connectStream, 1500);
            };
        }

        fetch('/api/jobs')
            .then(r => r.json())
            .then(data => renderCollections(data.collections || []))
            .catch(() => {});

        connectStream();

        function escapeHtml(s) {
            const div = document.createElement('div');
            div.textContent = String(s || '');
            return div.innerHTML;
        }
    </script>
</body>
</html>{{end}}`

// jobsPartial renders the job list.
const jobsPartial = `{{define "jobs"}}<ul id="job-list" class="jobs">
{{if .}}{{range .}}<li class="collection">
    <div class="collection-header">
        {{if and .CoverURL (not .PlaceholderCover)}}<img class="cover" src="{{.CoverURL}}" alt="cover">{{else}}<div class="cover cover-placeholder">Cover</div>{{end}}
        <div class="collection-main">
            <div class="collection-title">{{.Title}}</div>
            <div class="collection-meta">{{.Kind}} · {{.DoneTracks}}/{{.TotalTracks}} done</div>
            <div class="progress"><div class="progress-fill" style="width:0%"></div></div>
        </div>
    </div>
</li>{{end}}{{else}}<p class="empty">No jobs yet.</p>{{end}}
</ul>{{end}}`
