pub(crate) const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>SwiftPipe Harness</title>
  <style>
    :root {
      color-scheme: light;
      --bg: #f7f8f5;
      --ink: #1d2524;
      --muted: #66716d;
      --line: #d8ddd7;
      --panel: #ffffff;
      --accent: #176b62;
      --accent-ink: #ffffff;
      --warn: #9b4d18;
      --bad: #a13737;
      --code: #101817;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      background: var(--bg);
      color: var(--ink);
      font: 14px/1.45 system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }
    header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 16px;
      padding: 18px 24px;
      border-bottom: 1px solid var(--line);
      background: var(--panel);
    }
    h1 { margin: 0; font-size: 22px; font-weight: 700; }
    h2 { margin: 0 0 12px; font-size: 15px; }
    .header-links { display: flex; gap: 8px; align-items: center; }
    .header-links a {
      color: var(--accent);
      border: 1px solid var(--line);
      border-radius: 999px;
      padding: 6px 10px;
      text-decoration: none;
      background: #fff;
      font-weight: 650;
      font-size: 13px;
    }
    main { display: grid; grid-template-columns: minmax(360px, 0.9fr) minmax(420px, 1.1fr); gap: 18px; padding: 18px; }
    section, .panel {
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 8px;
      padding: 16px;
    }
    label { display: grid; gap: 6px; color: var(--muted); font-size: 12px; font-weight: 650; }
    input, textarea, select, button {
      width: 100%;
      border-radius: 6px;
      border: 1px solid var(--line);
      font: inherit;
    }
    input, textarea, select { padding: 9px 10px; background: #fff; color: var(--ink); }
    textarea {
      min-height: 300px;
      resize: vertical;
      font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      color: var(--code);
    }
    button {
      cursor: pointer;
      border-color: var(--accent);
      background: var(--accent);
      color: var(--accent-ink);
      font-weight: 700;
      padding: 10px 12px;
    }
    button.secondary { width: auto; background: #fff; color: var(--accent); }
    button[aria-selected="true"] { background: var(--accent); color: var(--accent-ink); }
    button[aria-selected="false"] { background: #fff; color: var(--accent); }
    pre {
      margin: 0;
      min-height: 220px;
      overflow: auto;
      white-space: pre-wrap;
      overflow-wrap: anywhere;
      border-radius: 6px;
      background: #111917;
      color: #edf4ef;
      padding: 12px;
      font: 12px/1.45 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    }
    .tabs { display: flex; gap: 8px; margin-bottom: 14px; }
    .tabs button { flex: 1; }
    .stack { display: grid; gap: 12px; }
    .row { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
    .results { display: grid; grid-template-rows: auto auto 1fr; gap: 14px; min-width: 0; }
    .status {
      min-height: 42px;
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      border: 1px solid var(--line);
      border-radius: 8px;
      background: #fff;
      padding: 10px 12px;
      color: var(--muted);
      font-weight: 650;
    }
    .status[data-state="completed"] { color: var(--accent); }
    .status[data-state="completed_with_errors"] { color: var(--warn); }
    .status[data-state="failed"], .status[data-state="error"] { color: var(--bad); }
    .links { display: flex; flex-wrap: wrap; gap: 8px; }
    .links a {
      color: var(--accent);
      border: 1px solid var(--line);
      border-radius: 999px;
      padding: 6px 10px;
      text-decoration: none;
      background: #fff;
      font-weight: 650;
    }
    .hidden { display: none; }
    @media (max-width: 920px) {
      header { align-items: flex-start; flex-direction: column; }
      main { grid-template-columns: 1fr; padding: 12px; }
      .row { grid-template-columns: 1fr; }
    }
  </style>
</head>
<body>
  <header>
    <h1>SwiftPipe Harness</h1>
    <div class="header-links">
      <div class="links" id="artifactLinks"></div>
      <a href="/docs" target="_blank">API Docs</a>
      <a href="/app/" target="_blank">Control Panel</a>
    </div>
  </header>
  <main>
    <section>
      <div class="tabs" role="tablist" aria-label="Job mode">
        <button id="uploadTab" class="secondary" role="tab" aria-selected="true">Upload</button>
        <button id="prefixTab" class="secondary" role="tab" aria-selected="false">Prefix Job</button>
      </div>
      <div id="uploadPanel" class="stack" role="tabpanel">
        <label>FIN file
          <input id="file" type="file">
        </label>
        <label>Message type
          <input id="messageType" placeholder="MT540">
        </label>
        <label>Outputs
          <select id="uploadOutputs">
            <option value="all">Full artifacts</option>
            <option value="rendered">Rendered FIN only</option>
            <option value="parquet,errors">Parquet and errors</option>
          </select>
        </label>
        <label>FIN message
          <textarea id="fin" placeholder="{1:...}{2:...}{4:...-}"></textarea>
        </label>
        <button id="upload">Upload and process</button>
      </div>
      <div id="prefixPanel" class="stack hidden" role="tabpanel">
        <label>Input prefix
          <input id="inputPrefix" placeholder="s3://swiftpipe-inbox/daily/">
        </label>
        <div class="row">
          <label>Include suffix
            <input id="includeSuffix" value=".fin">
          </label>
          <label>Message type
            <input id="prefixMessageType" placeholder="MT540">
          </label>
        </div>
        <label>Outputs
          <select id="prefixOutputs">
            <option value="all">Full artifacts</option>
            <option value="rendered">Rendered FIN only</option>
            <option value="parquet,errors">Parquet and errors</option>
          </select>
        </label>
        <button id="submitPrefix">Run prefix job</button>
      </div>
    </section>
    <div class="results">
      <div id="status" class="status" data-state="idle">
        <span id="statusText">Idle</span>
        <span id="countText"></span>
      </div>
      <div class="row">
        <section>
          <h2>Manifest</h2>
          <pre id="manifest"></pre>
        </section>
        <section>
          <h2>Rendered FIN</h2>
          <pre id="rendered"></pre>
        </section>
      </div>
    </div>
  </main>
  <script>
    const $ = (id) => document.getElementById(id);
    const objectUrl = (uri) => '/v1/object/' + encodeURIComponent(uri);

    function setMode(mode) {
      const upload = mode === 'upload';
      $('uploadPanel').classList.toggle('hidden', !upload);
      $('prefixPanel').classList.toggle('hidden', upload);
      $('uploadTab').setAttribute('aria-selected', upload ? 'true' : 'false');
      $('prefixTab').setAttribute('aria-selected', upload ? 'false' : 'true');
    }

    function setStatus(state, text, counts) {
      $('status').dataset.state = state;
      $('statusText').textContent = text;
      $('countText').textContent = counts || '';
    }

    function setLinks(manifest) {
      const links = $('artifactLinks');
      links.textContent = '';
      if (!manifest || !manifest.outputs) return;
      const items = [['Manifest', manifest.outputs.manifest]];
      if (manifest.timings && manifest.timings.export_ms > 0) items.push(['Errors', manifest.outputs.errors_ndjson]);
      if (manifest.timings && manifest.timings.zip_ms > 0) items.push(['Zip', manifest.outputs.zip]);
      for (const [label, uri] of items) {
        if (!uri) continue;
        const anchor = document.createElement('a');
        anchor.href = objectUrl(uri);
        anchor.textContent = label;
        anchor.target = '_blank';
        links.appendChild(anchor);
      }
    }

    async function showManifest(manifest) {
      $('manifest').textContent = JSON.stringify(manifest, null, 2);
      setLinks(manifest);
      const counts = manifest.counts
        ? `${manifest.counts.messages || 0} messages, ${manifest.counts.rendered || 0} rendered`
        : '';
      setStatus(manifest.status || 'error', manifest.status || 'Error', counts);
      const renderedUri = manifest.messages && manifest.messages.find((m) => m.rendered_uri)?.rendered_uri;
      if (renderedUri) {
        $('rendered').textContent = await fetch(objectUrl(renderedUri)).then((r) => r.text());
      } else {
        $('rendered').textContent = '';
      }
    }

    async function readJsonResponse(response) {
      const body = await response.json();
      if (!response.ok) throw new Error(body.error || response.statusText);
      return body;
    }

    async function pollUntilDone(jobId) {
      setStatus('running', `Job ${jobId} — queued…`, '');
      for (let i = 0; i < 600; i++) {
        await new Promise((r) => setTimeout(r, 1000));
        const view = await fetch(`/v1/jobs/${jobId}`).then(readJsonResponse);
        if (view.status === 'queued' || view.status === 'running') {
          setStatus('running', `Job ${jobId} — ${view.status}…`, '');
          continue;
        }
        if (view.status === 'failed') throw new Error(view.error || 'Job failed');
        // completed or completed_with_errors — fetch manifest
        const manifest = await fetch(`/v1/jobs/${jobId}/manifest`).then(readJsonResponse);
        await showManifest(manifest);
        return;
      }
      throw new Error('Timed out waiting for job to complete');
    }

    $('uploadTab').addEventListener('click', () => setMode('upload'));
    $('prefixTab').addEventListener('click', () => setMode('prefix'));
    $('file').addEventListener('change', async () => {
      if ($('file').files[0]) $('fin').value = await $('file').files[0].text();
    });

    $('upload').addEventListener('click', async () => {
      try {
        setStatus('running', 'Submitting…', '');
        const mt = $('messageType').value.trim();
        const params = new URLSearchParams();
        if (mt) params.set('message_type', mt);
        if ($('uploadOutputs').value !== 'all') params.set('outputs', $('uploadOutputs').value);
        const query = params.toString();
        const url = '/v1/upload' + (query ? '?' + query : '');
        const resp = await fetch(url, { method: 'POST', body: $('fin').value }).then(readJsonResponse);
        if (resp.job_id) {
          await pollUntilDone(resp.job_id);
        } else {
          await showManifest(resp);
        }
      } catch (err) {
        setStatus('error', err.message, '');
      }
    });

    $('submitPrefix').addEventListener('click', async () => {
      try {
        setStatus('running', 'Submitting…', '');
        const payload = {
          input_prefix: $('inputPrefix').value.trim(),
          include_suffix: $('includeSuffix').value.trim() || '.fin',
          message_type: $('prefixMessageType').value.trim() || undefined,
          render_validate: true,
          outputs: $('prefixOutputs').value === 'all' ? undefined : $('prefixOutputs').value.split(','),
        };
        const resp = await fetch('/v1/jobs', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify(payload),
        }).then(readJsonResponse);
        if (resp.job_id) {
          await pollUntilDone(resp.job_id);
        } else {
          await showManifest(resp);
        }
      } catch (err) {
        setStatus('error', err.message, '');
      }
    });
  </script>
</body>
</html>
"#;
