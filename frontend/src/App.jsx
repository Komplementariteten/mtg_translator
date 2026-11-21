import { useEffect, useState } from 'react'

const languageNames = {
  en: 'English', de: 'German', fr: 'French', es: 'Spanish', it: 'Italian',
  pt: 'Portuguese', ja: 'Japanese', ko: 'Korean', ru: 'Russian',
  zhs: 'Simplified Chinese', zht: 'Traditional Chinese', he: 'Hebrew',
  la: 'Latin', grc: 'Ancient Greek', ar: 'Arabic', sa: 'Sanskrit', ph: 'Phyrexian',
}

async function request(url, signal) {
  const response = await fetch(url, { signal })
  if (!response.ok) {
    const body = await response.json().catch(() => null)
    throw new Error(body?.error || `Request failed (${response.status}).`)
  }
  return response.json()
}

function CardDetails({ card, language }) {
  const text = card.printed_text ?? card.oracle_text
  return (
    <section className="card-face">
      <h3>{card.printed_name || card.name}</h3>
      {card.printed_name && card.printed_name !== card.name && <p className="muted">{card.name}</p>}
      {card.mana_cost && <p>{card.mana_cost}</p>}
      <p>{card.printed_type_line || card.type_line}</p>
      {text && (
        <>
          {language !== 'en' && !card.printed_text && (
            <p className="muted">English Oracle text (localized text unavailable):</p>
          )}
          <p className="rules">{text}</p>
        </>
      )}
    </section>
  )
}

function App() {
  const [name, setName] = useState('')
  const [language, setLanguage] = useState('')
  const [languages, setLanguages] = useState([])
  const [cards, setCards] = useState([])
  const [error, setError] = useState('')
  const [loadingLanguages, setLoadingLanguages] = useState(true)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    const controller = new AbortController()
    request('/api/languages', controller.signal)
      .then((available) => {
        setLanguages(available)
        setLanguage(available.includes('en') ? 'en' : available[0] || '')
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

  async function translate(event) {
    event.preventDefault()
    setLoading(true)
    setError('')
    setCards([])
    try {
      const query = new URLSearchParams({ name: name.trim(), lang: language })
      setCards(await request(`/api/translate?${query}`))
    } catch (failure) {
      setError(failure.message)
    } finally {
      setLoading(false)
    }
  }

  return (
    <main>
      <header>
        <p className="eyebrow">Local Scryfall database</p>
        <h1>Magic card translator</h1>
        <p>Find a card’s printed name and rules in another language. No online requests required.</p>
      </header>

      <form onSubmit={translate}>
        <label htmlFor="card-name">Card name</label>
        <input
          id="card-name"
          value={name}
          onChange={(event) => { setName(event.target.value); setCards([]) }}
          placeholder="e.g. Lightning Bolt or Blitzschlag"
          required
          maxLength={300}
          disabled={loading}
        />
        <p className="muted">Use an exact English or localized name, including an individual card face.</p>
        <label htmlFor="language">Translate to</label>
        <select
          id="language"
          value={language}
          onChange={(event) => { setLanguage(event.target.value); setCards([]) }}
          disabled={loadingLanguages || loading || !languages.length}
        >
          {loadingLanguages && <option value="">Loading languages…</option>}
          {!loadingLanguages && !languages.length && <option value="">No languages available</option>}
          {languages.map((code) => (
            <option key={code} value={code}>{languageNames[code] || code} ({code})</option>
          ))}
        </select>
        <button disabled={loading || !language || !name.trim()}>
          {loading ? 'Translating…' : 'Translate card'}
        </button>
      </form>

      {error && <p role="alert" className="error">{error}</p>}
      <section aria-live="polite" aria-busy={loading} aria-label="Translation results">
        {loading && <p>Looking up the local database…</p>}
        {cards.map((card) => (
          <article className="card" key={card.id}>
            <h2>{card.printed_name || card.card_faces.map((face) => face.printed_name || face.name).join(' // ') || card.name}</h2>
            <p className="muted">{languageNames[card.lang] || card.lang} printing · {card.set_name || 'Unknown set'} {card.collector_number}</p>
            {(card.card_faces.length ? card.card_faces : [card]).map((face, index) => (
              <CardDetails key={index} card={face} language={card.lang} />
            ))}
          </article>
        ))}
      </section>
      <footer>
        Translations are existing card printings, not machine translations.
        Availability depends on your local database.
      </footer>
    </main>
  )
}

export default App
