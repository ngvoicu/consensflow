/** `text` as HTML holds it. */
export function escape(text) {
  return String(text ?? '')
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
}

/** A page of the site: its title, a link back to the index, and its body. */
export function page(title, body) {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>${escape(title)} · ledgerline</title>
<link rel="stylesheet" href="style.css">
</head>
<body>
<p><a href="index.html">ledgerline</a></p>
<h1>${escape(title)}</h1>
${body}
</body>
</html>
`
}

/** A table: its head, and its rows, each a list of cells already escaped. */
export function table(head, rows) {
  const cells = (tag, row) => row.map((cell) => `<${tag}>${cell}</${tag}>`).join('')
  return `<table>
<thead><tr>${cells('th', head.map(escape))}</tr></thead>
<tbody>
${rows.map((row) => `<tr>${cells('td', row)}</tr>`).join('\n')}
</tbody>
</table>`
}

/** A list of links: each a page and its words. */
export function links(pages) {
  return `<ul>\n${pages.map(([href, words]) => `<li><a href="${escape(href)}">${escape(words)}</a></li>`).join('\n')}\n</ul>`
}
