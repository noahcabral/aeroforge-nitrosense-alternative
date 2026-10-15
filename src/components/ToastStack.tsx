export type AppToast = {
  id: string
  tone: 'info' | 'success' | 'warning' | 'error'
  title: string
  detail: string
}

type ToastStackProps = {
  toasts: AppToast[]
  onDismiss: (id: string) => void
}

export function ToastStack({ toasts, onDismiss }: ToastStackProps) {
  if (toasts.length === 0) {
    return null
  }

  return (
    <div className="toast-stack" aria-live="polite" aria-relevant="additions text">
      {toasts.map((toast) => (
        <section key={toast.id} className={`app-toast app-toast--${toast.tone}`}>
          <div>
            <strong>{toast.title}</strong>
            <p>{toast.detail}</p>
          </div>
          <button type="button" aria-label="Dismiss notification" onClick={() => onDismiss(toast.id)}>
            &times;
          </button>
        </section>
      ))}
    </div>
  )
}
