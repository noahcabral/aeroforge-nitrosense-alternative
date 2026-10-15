import type { FanCurveTemplate } from '../lib/curves'

type CurveTemplateBarProps = {
  templates: FanCurveTemplate[]
  disabled?: boolean
  onApply: (template: FanCurveTemplate) => void
}

export function CurveTemplateBar({ templates, disabled, onApply }: CurveTemplateBarProps) {
  return (
    <div className="curve-template-bar">
      <div className="curve-template-bar__copy">
        <span className="eyebrow">Curve templates</span>
        <strong>Start from a known-good Custom shape</strong>
        <p>Loads into Custom mode and applies when hardware is ready. Edit nodes afterward.</p>
      </div>
      <div className="curve-template-bar__actions">
        {templates.map((template) => (
          <button
            key={template.id}
            type="button"
            className="button button--ghost curve-template-bar__button"
            disabled={disabled}
            title={template.summary}
            onClick={() => onApply(template)}
          >
            <strong>{template.name}</strong>
            <small>{template.strap}</small>
          </button>
        ))}
      </div>
    </div>
  )
}
