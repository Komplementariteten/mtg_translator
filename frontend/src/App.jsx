import { useEffect, useRef, useState } from 'react'

const languageNames = {
  en: 'English', de: 'German', fr: 'French', es: 'Spanish', it: 'Italian',
  pt: 'Portuguese', ja: 'Japanese', ko: 'Korean', ru: 'Russian',
  zhs: 'Simplified Chinese', zht: 'Traditional Chinese', he: 'Hebrew',
  la: 'Latin', grc: 'Ancient Greek', ar: 'Arabic', sa: 'Sanskrit', ph: 'Phyrexian',
  tlh: 'Klingon', qya: 'Quenya', dw: 'Dwarvish',
}

async function request(url, signal) {
  const response = await fetch(url, { signal })
  if (!response.ok) {
    const body = await response.json().catch(() => null)
    throw new Error(body?.error || `Request failed (${response.status}).`)
  }
  return response.json()
}

const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

// Scryfall has no keyword glossary, so explanations come from the reminder text in the Oracle text.
function keywordHints(text, keywords) {
  if (!text) return []
  return keywords.flatMap((keyword) => {
    const match = text.match(new RegExp(`${escapeRegExp(keyword)}[^\\n(]*\\(([^)]+)\\)`, 'i'))
    return match ? [{ keyword, hint: match[1] }] : []
  })
}

const manaColors = { W: '#f2ecc8', U: '#4a90d9', B: '#5b4d66', R: '#d9533f', G: '#3f9e5a', C: '#aab0b8' }

function ManaSymbol({ symbol }) {
  const parts = symbol.split('/')
  const colors = parts.filter((part) => manaColors[part]).map((part) => manaColors[part])
  const background = colors.length > 1
    ? `linear-gradient(135deg, ${colors[0]} 50%, ${colors[1]} 50%)`
    : colors[0] || '#c9ccd1'
  const label = parts.includes('P') ? 'Φ' : colors.length === 1 && manaColors[parts[0]] ? parts[0] : symbol
  return <span className="mana-symbol" style={{ background, color: colors.length === 1 && !['#f2ecc8', '#aab0b8'].includes(colors[0]) ? '#fff' : undefined }} title={`{${symbol}}`}>{label}</span>
}

function ManaCost({ cost }) {
  const symbols = [...cost.matchAll(/\{([^}]+)\}/g)].map((match) => match[1])
  return (
    <span className="mana-cost" role="img" aria-label={cost}>
      {symbols.map((symbol, index) => <ManaSymbol key={index} symbol={symbol} />)}
    </span>
  )
}

function CardImage({ card, index }) {
  const [missing, setMissing] = useState(false)
  if (missing || !card.oracle_id) return null
  return (
    <img
      className="card-image"
      src={`/images/${card.oracle_id}-${index}.jpg`}
      alt={`Card image of ${card.name}`}
      width="244"
      height="340"
      onError={() => setMissing(true)}
    />
  )
}

function Printings({ printings }) {
  if (!printings?.length) return null
  return (
    <details className="printings">
      <summary>All printings, oldest first ({printings.length})</summary>
      <ul>
        {printings.map((printing) => (
          <li key={printing.id}>
            <strong>{printing.set_name || 'Unknown set'}</strong>
            {printing.set && <span className="muted"> ({printing.set.toUpperCase()})</span>}
            {printing.collector_number && <span> #{printing.collector_number}</span>}
            <span className="muted">
              {' · '}{printing.lang}{printing.released_at && ` · ${printing.released_at}`}{printing.rarity && ` · ${printing.rarity}`}
            </span>
          </li>
        ))}
      </ul>
    </details>
  )
}

function CardDetails({ card, keywords, image }) {
  const hints = keywordHints(card.oracle_text, keywords)
  return (
    <section className="card-face">
      {image}
      <div className="face-text">
        <h3>{card.name}</h3>
        {card.mana_cost && <p><ManaCost cost={card.mana_cost} /></p>}
        <p>{card.type_line}</p>
        {card.oracle_text && <p className="rules">{card.oracle_text}</p>}
        {hints.length > 0 && (
          <dl className="keywords">
            {hints.map(({ keyword, hint }) => (
              <div key={keyword}>
                <dt>{keyword}</dt>
                <dd>{hint}</dd>
              </div>
            ))}
          </dl>
        )}
      </div>
    </section>
  )
}

function App() {
  const [name, setName] = useState('')
  const [language, setLanguage] = useState('')
  const [languages, setLanguages] = useState([])
  const [loadingLanguages, setLoadingLanguages] = useState(true)
  const [suggestions, setSuggestions] = useState([])
  const [activeIndex, setActiveIndex] = useState(-1)
  const [dismissed, setDismissed] = useState(false)
  const inputRef = useRef(null)
  const [cards, setCards] = useState([])
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    const controller = new AbortController()
    request('/api/languages', controller.signal)
      .then((available) => {
        setLanguages(available)
        setLanguage(available.includes('de') ? 'de' : available[0] || '')
        setLoadingLanguages(false)
      })
      .catch((failure) => {
        if (failure.name !== 'AbortError') {
          setError(`Could not load languages. Check that the backend is running. ${failure.message}`)
          setLoadingLanguages(false)
        }
      })
    return () => controller.abort()
  }, [])

  useEffect(() => {
    if (name.trim().length < 3 || !language) return undefined
    const controller = new AbortController()
    const timer = setTimeout(() => {
      const query = new URLSearchParams({ q: name.trim(), lang: language })
      request(`/api/suggest?${query}`, controller.signal)
        .then((list) => { setSuggestions(list); setActiveIndex(-1) })
        .catch(() => {})
    }, 200)
    return () => { clearTimeout(timer); controller.abort() }
  }, [name, language])

  async function lookup(cardName) {
    setLoading(true)
    setError('')
    setCards([])
    setSuggestions([])
    try {
      const query = new URLSearchParams({ name: cardName.trim(), lang: 'en' })
      setCards(await request(`/api/translate?${query}`))
    } catch (failure) {
      setError(failure.message)
    } finally {
      setLoading(false)
      setTimeout(() => inputRef.current?.focus(), 0)
    }
  }

  function translate(event) {
    event.preventDefault()
    lookup(name)
  }

  const listOpen = !dismissed && name.trim().length >= 3 && suggestions.length > 0

  function handleKeyDown(event) {
    if (event.key === 'Escape') {
      setDismissed(true)
      setActiveIndex(-1)
    } else if (listOpen && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) {
      event.preventDefault()
      const step = event.key === 'ArrowDown' ? 1 : -1
      setActiveIndex((index) => (index + step + suggestions.length + (index < 0 && step < 0 ? 1 : 0)) % suggestions.length)
    } else if (event.key === 'ArrowDown' && suggestions.length > 0) {
      setDismissed(false)
    } else if (event.key === 'Enter' && listOpen && activeIndex >= 0) {
      event.preventDefault()
      pick(suggestions[activeIndex].name)
    }
  }

  function pick(suggestion) {
    setName(suggestion)
    lookup(suggestion)
  }

  return (
    <main>
      <header>
        <p className="eyebrow">Local Scryfall database</p>
        <h1>Magic card translator</h1>
        <p>Choose the language of a card name to find its English name and rules. No online requests required.</p>
      </header>

      <form onSubmit={translate}>
        <div className="search-row">
          <div className="field field-language">
            <label htmlFor="language">Language</label>
            <select
              id="language"
              value={language}
              onChange={(event) => { setLanguage(event.target.value); setSuggestions([]); setCards([]) }}
              disabled={loadingLanguages || loading || !languages.length}
            >
              {loadingLanguages && <option value="">Loading languages…</option>}
              {!loadingLanguages && !languages.length && <option value="">No languages available</option>}
              {languages.map((code) => (
                <option key={code} value={code}>{code} · {languageNames[code] || code}</option>
              ))}
            </select>
          </div>
          <div className="field field-name">
            <label htmlFor="card-name">Card name</label>
            <input
              id="card-name"
              value={name}
              onChange={(event) => { setName(event.target.value); setCards([]); setDismissed(false); setActiveIndex(-1) }}
              placeholder="e.g. Blitzschlag"
              required
              maxLength={300}
              disabled={loading}
              autoComplete="off"
              autoCapitalize="off"
              spellCheck={false}
              enterKeyHint="search"
              ref={inputRef}
              onKeyDown={handleKeyDown}
              role="combobox"
              aria-expanded={listOpen}
              aria-controls="suggestions"
              aria-autocomplete="list"
              aria-activedescendant={listOpen && activeIndex >= 0 ? `suggestion-${activeIndex}` : undefined}
            />
          </div>
        </div>
        {listOpen && (
          <ul id="suggestions" className="suggestions" role="listbox" aria-label="Card name suggestions">
            {suggestions.map((suggestion, index) => (
              <li key={suggestion.name} role="presentation">
                <button
                  type="button"
                  id={`suggestion-${index}`}
                  role="option"
                  aria-selected={index === activeIndex}
                  tabIndex={-1}
                  className={`suggestion color-${suggestion.color}`}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => pick(suggestion.name)}
                >
                  <span>{suggestion.name}</span>
                  {suggestion.mana_cost && <ManaCost cost={suggestion.mana_cost} />}
                </button>
              </li>
            ))}
          </ul>
        )}
        <p className="muted">Suggestions appear after 3 characters and match the start of any word (ae, oe, ue also match ä, ö, ü). Use an exact name, including an individual card face.</p>
        <button disabled={loading || !language || !name.trim()}>
          {loading ? 'Searching…' : 'Show English card'}
        </button>
      </form>

      {error && <p role="alert" className="error">{error}</p>}
      <section aria-live="polite" aria-busy={loading} aria-label="Translation results">
        {loading && <p>Looking up the local database…</p>}
        {cards.map((card) => (
          <article className="card" key={card.id}>
            <h2>{card.name}</h2>
            <p className="muted">English printing · {card.set_name || 'Unknown set'} {card.collector_number}</p>
            {(() => {
              const faceImages = card.card_faces.some((face) => face.image_uris)
              return (card.card_faces.length ? card.card_faces : [card]).map((face, index) => (
                <CardDetails
                  key={index}
                  card={face}
                  keywords={card.keywords}
                  image={<CardImage card={card} index={faceImages ? index : 0} />}
                />
              ))
            })()}
            <Printings printings={card.printings} />
          </article>
        ))}
      </section>
      <footer>
        Results are existing English card printings, not machine translations.
        Availability depends on your local database.
      </footer>
    </main>
  )
}

export default App
