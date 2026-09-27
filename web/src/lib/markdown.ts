import DOMPurify from 'dompurify'
import { Marked, type TokenizerAndRendererExtension } from 'marked'

// Ticket keys like KAN-12 in comments and descriptions become links that open
// the ticket panel. Agents write these constantly, so it's worth linking them.
const ticketRef: TokenizerAndRendererExtension = {
  name: 'ticketRef',
  level: 'inline',
  start(src) {
    return src.match(/\b[A-Z][A-Z0-9]{1,9}-\d+\b/)?.index
  },
  tokenizer(src) {
    const m = /^[A-Z][A-Z0-9]{1,9}-\d+\b/.exec(src)
    if (m) return { type: 'ticketRef', raw: m[0], key: m[0] }
    return undefined
  },
  renderer(token) {
    return `<a class="ticket-ref" href="?ticket=${token.key}" data-ticket="${token.key}">${token.key}</a>`
  },
}

const marked = new Marked({ gfm: true, breaks: true, extensions: [ticketRef] })

DOMPurify.addHook('afterSanitizeAttributes', (node) => {
  // External links open in a new tab and can't reach back into this window.
  if (node.tagName === 'A' && !node.hasAttribute('data-ticket')) {
    node.setAttribute('target', '_blank')
    node.setAttribute('rel', 'noopener noreferrer')
  }
})

/** Markdown to sanitized HTML. Everything user- or agent-written goes through this. */
export function renderMarkdown(src: string): string {
  const html = marked.parse(src, { async: false }) as string
  return DOMPurify.sanitize(html, { ADD_ATTR: ['target', 'data-ticket'] })
}
