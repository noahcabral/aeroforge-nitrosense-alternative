import { useEffect, useState } from 'react'
import {
  getAppRules,
  saveAppRules,
  type AppRule,
  type FanProfileId,
  type PowerProfileId,
} from '../lib/backend'
import { describeError } from '../lib/errors'

type Option<T> = { id: T; name: string }

type AppRulesPanelProps = {
  powerOptions: Option<PowerProfileId>[]
  fanOptions: Option<FanProfileId>[]
}

export function AppRulesPanel({ powerOptions, fanOptions }: AppRulesPanelProps) {
  const [rules, setRules] = useState<AppRule[]>([])
  const [draft, setDraft] = useState('')
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    getAppRules()
      .then(setRules)
      .catch((reason: unknown) => setError(describeError(reason)))
  }, [])

  async function commit(next: AppRule[]) {
    try {
      setRules(await saveAppRules(next))
      setError(null)
    } catch (reason) {
      setError(describeError(reason))
    }
  }

  function update(index: number, change: Partial<AppRule>) {
    void commit(rules.map((rule, i) => (i === index ? { ...rule, ...change } : rule)))
  }

  function add() {
    if (!draft.trim()) return
    void commit([...rules, { exeName: draft, powerProfile: 'turbo', fanProfile: 'max' }])
    setDraft('')
  }

  return (
    <div className="personal-setting-block app-rules">
      <div>
        <strong>App Profiles</strong>
        <p>
          While a listed app runs, AeroForge switches to its power and fan modes, then returns
          to your current defaults when it closes. The first listed app wins if several run.
        </p>
        {error && <small>{error}</small>}
      </div>

      {rules.map((rule, index) => (
        <div className="app-rules__row" key={rule.exeName}>
          <span className="app-rules__exe">{rule.exeName}</span>
          <label className="power-select">
            <span className="eyebrow">Power</span>
            <select
              value={rule.powerProfile}
              onChange={(event) =>
                update(index, { powerProfile: event.target.value as PowerProfileId })
              }
            >
              {powerOptions.map((option) => (
                <option key={option.id} value={option.id}>
                  {option.name}
                </option>
              ))}
            </select>
          </label>
          <label className="power-select">
            <span className="eyebrow">Fan</span>
            <select
              value={rule.fanProfile}
              onChange={(event) =>
                update(index, { fanProfile: event.target.value as FanProfileId })
              }
            >
              {fanOptions.map((option) => (
                <option key={option.id} value={option.id}>
                  {option.name}
                </option>
              ))}
            </select>
          </label>
          <button
            className="button button--ghost"
            type="button"
            onClick={() => void commit(rules.filter((_, i) => i !== index))}
          >
            Remove
          </button>
        </div>
      ))}

      <form
        className="app-rules__row"
        onSubmit={(event) => {
          event.preventDefault()
          add()
        }}
      >
        <input
          className="power-setting__input"
          placeholder="game.exe"
          aria-label="App executable name"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
        />
        <button className="button" type="submit" disabled={!draft.trim()}>
          Add App
        </button>
      </form>
    </div>
  )
}
