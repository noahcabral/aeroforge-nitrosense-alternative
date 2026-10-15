type ProcessorStateControlProps = {
  label: string
  min?: number
  value: number
  onChange: (value: number) => void
}

export function ProcessorStateControl({
  label,
  min = 5,
  value,
  onChange,
}: ProcessorStateControlProps) {
  return (
    <label className="power-setting">
      <div className="power-setting__header">
        <span>{label}</span>
        <strong>{value}%</strong>
      </div>

      <input
        className="power-setting__slider"
        type="range"
        min={min}
        max="100"
        step="1"
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
      />

      <input
        className="power-setting__input"
        type="number"
        min={min}
        max="100"
        step="1"
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
      />
    </label>
  )
}

