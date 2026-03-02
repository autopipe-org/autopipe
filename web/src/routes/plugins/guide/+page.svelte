<svelte:head>
	<title>Plugin Creation Guide - AutoPipe</title>
</svelte:head>

<main>
	<div class="guide">
		<a href="/plugins" class="back-link">&larr; Back to Plugins</a>

		<h1 class="guide-title">Plugin Creation Guide</h1>
		<p class="guide-intro">
			AutoPipe 플러그인은 Results Viewer를 확장하여 추가 파일 형식의 미리보기를 제공합니다.
			플러그인은 HTML/JavaScript 기반으로 동작하며, 브라우저에서 실행됩니다.
		</p>

		<!-- Section 1: What is a Plugin -->
		<section class="guide-section">
			<h2>1. 플러그인이란?</h2>
			<p>
				AutoPipe의 Results Viewer에는 이미지, PDF, 텍스트, BAM/VCF(igv.js), h5ad(jsfive) 등의
				내장 뷰어가 있습니다. 플러그인을 사용하면 이 외에도 사용자 정의 파일 형식을 지원할 수 있습니다.
			</p>
			<p>
				예를 들어, <code>.xyz</code> 형식의 커스텀 시각화가 필요한 경우 해당 형식을 위한 플러그인을 만들 수 있습니다.
			</p>
		</section>

		<!-- Section 2: Plugin Structure -->
		<section class="guide-section">
			<h2>2. 플러그인 구조</h2>
			<div class="code-block">
				<pre>{`my-viewer-plugin/
├── manifest.json    # 플러그인 메타데이터 (필수)
├── index.js         # 메인 진입점 (필수)
├── style.css        # 스타일시트 (선택)
└── lib/             # 추가 라이브러리 (선택)`}</pre>
			</div>

			<h3>manifest.json</h3>
			<div class="code-block">
				<pre>{`{
  "name": "my-viewer-plugin",
  "version": "1.0.0",
  "description": "Custom viewer for .xyz files",
  "author": "your-github-username",
  "extensions": ["xyz", "abc"],
  "entry": "index.js",
  "style": "style.css"
}`}</pre>
			</div>
			<table class="field-table">
				<thead>
					<tr><th>필드</th><th>필수</th><th>설명</th></tr>
				</thead>
				<tbody>
					<tr><td><code>name</code></td><td>O</td><td>플러그인 고유 이름</td></tr>
					<tr><td><code>version</code></td><td>O</td><td>시맨틱 버전 (예: 1.0.0)</td></tr>
					<tr><td><code>description</code></td><td></td><td>플러그인 설명</td></tr>
					<tr><td><code>extensions</code></td><td>O</td><td>지원하는 파일 확장자 배열</td></tr>
					<tr><td><code>entry</code></td><td>O</td><td>메인 JavaScript 파일 경로</td></tr>
					<tr><td><code>style</code></td><td></td><td>CSS 파일 경로 (선택)</td></tr>
				</tbody>
			</table>
		</section>

		<!-- Section 3: Plugin JS API -->
		<section class="guide-section">
			<h2>3. Plugin JavaScript API</h2>
			<p>
				플러그인의 entry 파일(<code>index.js</code>)에서 <code>window.AutoPipePlugin</code> 객체를 정의합니다:
			</p>
			<div class="code-block">
				<pre>{`window.AutoPipePlugin = {
  // 필수: 파일 렌더링
  render: function(container, fileUrl, filename) {
    // container: 렌더링할 DOM element
    // fileUrl: 파일 데이터 URL (예: "/file/result.xyz")
    // filename: 파일명 (예: "result.xyz")

    fetch(fileUrl)
      .then(resp => resp.text())
      .then(data => {
        container.innerHTML = '<pre>' + data + '</pre>';
      });
  },

  // 선택: 정리 (다른 파일 선택 시 호출)
  destroy: function() {
    // 이벤트 리스너 해제 등
  }
};`}</pre>
			</div>
			<p>
				<code>render()</code>는 사용자가 해당 확장자의 파일을 선택할 때 호출됩니다.
				<code>container</code>에 원하는 HTML을 렌더링하면 됩니다.
				<code>fileUrl</code>로 파일 데이터를 <code>fetch()</code>해서 사용할 수 있습니다.
			</p>
		</section>

		<!-- Section 4: Development & Testing -->
		<section class="guide-section">
			<h2>4. 개발 & 테스트</h2>
			<ol class="step-list">
				<li>
					<strong>플러그인 디렉토리에 파일 생성</strong>
					<p>
						기본 위치: <code>~/.local/share/autopipe/plugins/my-plugin/</code>
						(앱 설정에서 변경 가능)
					</p>
				</li>
				<li>
					<strong><code>manifest.json</code>과 <code>index.js</code> 작성</strong>
				</li>
				<li>
					<strong>AutoPipe에서 <code>show_results</code> 실행</strong>
					<p>결과 뷰어에서 해당 확장자의 파일이 플러그인으로 렌더링되는지 확인합니다.</p>
				</li>
			</ol>
		</section>

		<!-- Section 5: GitHub Account -->
		<section class="guide-section">
			<h2>5. GitHub 계정 준비</h2>
			<p>
				플러그인을 레지스트리에 등록하려면 GitHub 계정이 필요합니다.
				계정이 없으면 <a href="https://github.com/signup" target="_blank" rel="noopener">github.com/signup</a>에서 생성하세요.
			</p>
		</section>

		<!-- Section 6: GitHub Token -->
		<section class="guide-section">
			<h2>6. GitHub Personal Access Token 발급</h2>
			<ol class="step-list">
				<li>
					<a href="https://github.com/settings/tokens" target="_blank" rel="noopener">GitHub Settings &rarr; Developer settings &rarr; Personal access tokens &rarr; Tokens (classic)</a> 접속
				</li>
				<li><strong>Generate new token (classic)</strong> 클릭</li>
				<li>
					설정:
					<ul>
						<li>Note: <code>autopipe-plugin</code></li>
						<li>Expiration: 90일 권장</li>
						<li>Scopes: <code>public_repo</code>만 체크</li>
					</ul>
				</li>
				<li><strong>Generate token</strong> 클릭</li>
				<li><code>ghp_...</code>로 시작하는 토큰을 복사 (한 번만 표시됨)</li>
			</ol>
			<div class="callout">
				이 토큰은 레지스트리에 플러그인을 등록할 때 작성자 확인 및 GitHub 저장소 접근에 사용됩니다.
				토큰은 서버에 저장되지 않습니다.
			</div>
		</section>

		<!-- Section 7: GitHub Repository -->
		<section class="guide-section">
			<h2>7. GitHub 저장소 생성</h2>
			<ol class="step-list">
				<li>GitHub에서 새 public 저장소를 생성합니다.</li>
				<li>플러그인 파일들을 push합니다:
					<div class="code-block">
						<pre>{`cd my-viewer-plugin
git init
git add .
git commit -m "Initial plugin"
git remote add origin https://github.com/username/my-viewer-plugin.git
git push -u origin main`}</pre>
					</div>
				</li>
			</ol>
		</section>

		<!-- Section 8: Packaging & Publishing -->
		<section class="guide-section">
			<h2>8. 패키징 & 퍼블리시</h2>
			<ol class="step-list">
				<li>
					<strong>CLI 도구 설치</strong>
					<div class="code-block">
						<pre>npm install -g @pnucolab/autopipe-ext</pre>
					</div>
				</li>
				<li>
					<strong>검증</strong>
					<div class="code-block">
						<pre>autopipe-ext package</pre>
					</div>
					<p>manifest.json 유효성, entry 파일 존재, AutoPipePlugin 패턴 확인</p>
				</li>
				<li>
					<strong>퍼블리시</strong>
					<div class="code-block">
						<pre>{`autopipe-ext publish --token ghp_xxx
# 또는 환경변수: GITHUB_TOKEN=ghp_xxx autopipe-ext publish
# 또는 대화형 입력: autopipe-ext publish`}</pre>
					</div>
					<p>git remote에서 GitHub URL을 자동 감지하여 레지스트리에 등록합니다.</p>
				</li>
			</ol>
		</section>

		<!-- Section 9: Example -->
		<section class="guide-section">
			<h2>9. 예제: CSV Heatmap 플러그인</h2>
			<h3>manifest.json</h3>
			<div class="code-block">
				<pre>{`{
  "name": "csv-heatmap-viewer",
  "version": "1.0.0",
  "description": "Display CSV data as a color heatmap",
  "extensions": ["csv"],
  "entry": "index.js"
}`}</pre>
			</div>
			<h3>index.js</h3>
			<div class="code-block">
				<pre>{`window.AutoPipePlugin = {
  render: function(container, fileUrl, filename) {
    fetch(fileUrl)
      .then(function(resp) { return resp.text(); })
      .then(function(text) {
        var lines = text.trim().split('\\n');
        var headers = lines[0].split(',');
        var html = '<table style="border-collapse:collapse;font-size:12px;">';
        html += '<tr>' + headers.map(function(h) {
          return '<th style="padding:4px 8px;border:1px solid #ddd;">' + h + '</th>';
        }).join('') + '</tr>';
        for (var i = 1; i < Math.min(lines.length, 100); i++) {
          var cells = lines[i].split(',');
          html += '<tr>' + cells.map(function(c) {
            var n = parseFloat(c);
            var bg = isNaN(n) ? '#fff' :
              'hsl(' + Math.max(0, Math.min(240, 240 - n * 2.4)) + ',70%,85%)';
            return '<td style="padding:4px 8px;border:1px solid #eee;background:' + bg + '">' + c + '</td>';
          }).join('') + '</tr>';
        }
        html += '</table>';
        container.innerHTML = html;
      });
  }
};`}</pre>
			</div>
		</section>
	</div>
</main>

<style>
	.guide {
		max-width: 800px;
		margin: 0 auto;
		padding: 24px;
	}
	.back-link {
		display: inline-block;
		margin-bottom: 16px;
		color: #666;
		text-decoration: none;
		font-size: 14px;
	}
	.back-link:hover {
		color: #0366d6;
	}
	.guide-title {
		font-size: 28px;
		font-weight: 700;
		margin-bottom: 8px;
		color: #111;
	}
	.guide-intro {
		font-size: 15px;
		color: #555;
		line-height: 1.7;
		margin-bottom: 32px;
	}
	.guide-section {
		margin-bottom: 36px;
	}
	.guide-section h2 {
		font-size: 20px;
		font-weight: 700;
		color: #111;
		margin-bottom: 12px;
		padding-bottom: 8px;
		border-bottom: 1px solid #eee;
	}
	.guide-section h3 {
		font-size: 15px;
		font-weight: 600;
		color: #333;
		margin: 16px 0 8px;
	}
	.guide-section p {
		font-size: 14px;
		color: #444;
		line-height: 1.7;
		margin-bottom: 8px;
	}
	.guide-section a {
		color: #0366d6;
	}
	.code-block {
		background: #f6f8fa;
		border: 1px solid #e5e5e5;
		border-radius: 8px;
		padding: 14px 16px;
		margin: 8px 0 12px;
		overflow-x: auto;
	}
	.code-block pre {
		font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace;
		font-size: 13px;
		line-height: 1.5;
		color: #24292f;
		white-space: pre;
		margin: 0;
	}
	.field-table {
		width: 100%;
		border-collapse: collapse;
		font-size: 13px;
		margin: 12px 0;
	}
	.field-table th {
		background: #f6f8fa;
		padding: 8px 12px;
		text-align: left;
		font-weight: 600;
		border-bottom: 2px solid #e5e5e5;
	}
	.field-table td {
		padding: 6px 12px;
		border-bottom: 1px solid #f0f0f0;
	}
	.field-table code {
		background: #eff1f3;
		padding: 2px 6px;
		border-radius: 4px;
		font-size: 12px;
	}
	.step-list {
		padding-left: 24px;
		margin: 8px 0;
	}
	.step-list li {
		font-size: 14px;
		color: #444;
		line-height: 1.7;
		margin-bottom: 12px;
	}
	.step-list li strong {
		color: #111;
	}
	.step-list li p {
		margin: 4px 0 0;
		font-size: 13px;
		color: #666;
	}
	.step-list ul {
		padding-left: 20px;
		margin: 4px 0;
	}
	.step-list ul li {
		margin-bottom: 2px;
		font-size: 13px;
	}
	.callout {
		background: #fff8e1;
		border: 1px solid #ffe082;
		border-radius: 8px;
		padding: 12px 16px;
		font-size: 13px;
		color: #5d4037;
		margin: 12px 0;
		line-height: 1.6;
	}
</style>
