---
id: evil
title: <img src=x onerror="window.__pwned='frontmatter'">
status: active
---

# Evil <script>window.__pwned = 'heading'</script>

<script>window.__pwned = 'script'</script>
<img src=x onerror="window.__pwned='img'">
<iframe src="javascript:window.__pwned='iframe'"></iframe>

A [script link](javascript:window.__pwned='link'), a [data link](data:text/html,<script>alert(1)</script>),
a [link out of the folder](../../../etc/passwd), an [absolute one](/etc/passwd), a
[protocol-relative one](//evil.example/x) and an image ![pixel](https://evil.example/pixel.png).

- [x] <img src=x onerror="window.__pwned='task'"> a done task
- [ ] **<svg onload="window.__pwned='svg'">** an open one

| <img src=x onerror="window.__pwned='cell'"> | `<script>window.__pwned='code'</script>` |
|---|---|
| [x](javascript:window.__pwned='cell-link') | <a href="javascript:void(0)">raw</a> |

```mermaid
graph TD
  A["<img src=x onerror=window.__pwned='mermaid-label'>"] --> B["<script>window.__pwned='mermaid-script'</script>"]
  B --> C["<img src=https://evil.example/label.png> <span style='background:url(https://evil.example/span.png)'>styled</span>"]
  click A href "javascript:window.__pwned='mermaid-click'"
  click B call alert()
```

```mermaid
%%{init: {"themeCSS": ".node rect { fill: url(https://evil.example/leak.png); } @import url(https://evil.example/leak.css);", "fontFamily": "x; background: url(https://evil.example/font.png)"}}%%
graph LR
  X[Leak] --> Y[Here]
```

```mermaid
%%{init: {"theme": "forest", "themeVariables": {"primaryColor": "#ff0000", "mainBkg": "#ff0000"}, "themeCSS": ".node rect, .node polygon { fill: #00ff00 !important; }"}}%%
graph LR
  P[Its own colours] --> Q[Asked for]
```
