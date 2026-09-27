import { ApiError } from './api'

export interface Toast {
  id: number
  kind: 'info' | 'error' | 'success'
  message: string
}

let next = 1

class Toasts {
  items = $state<Toast[]>([])

  show(message: string, kind: Toast['kind'] = 'info', ms = 4000) {
    const id = next++
    this.items.push({ id, kind, message })
    setTimeout(() => this.dismiss(id), kind === 'error' ? Math.max(ms, 7000) : ms)
  }

  error(e: unknown) {
    const message = e instanceof ApiError || e instanceof Error ? e.message : String(e)
    this.show(message, 'error')
  }

  dismiss(id: number) {
    this.items = this.items.filter((t) => t.id !== id)
  }
}

export const toasts = new Toasts()

interface ConfirmRequest {
  title: string
  message: string
  confirm: string
  danger: boolean
  resolve: (ok: boolean) => void
}

class Confirmer {
  current = $state<ConfirmRequest | null>(null)

  ask(title: string, message: string, opts: { confirm?: string; danger?: boolean } = {}): Promise<boolean> {
    return new Promise((resolve) => {
      this.current = {
        title,
        message,
        confirm: opts.confirm ?? 'Confirm',
        danger: opts.danger ?? false,
        resolve,
      }
    })
  }

  answer(ok: boolean) {
    this.current?.resolve(ok)
    this.current = null
  }
}

export const confirmer = new Confirmer()
