import { createContext } from 'react'

type ToastType = 'success' | 'error' | 'info'

export interface ToastContextValue {
  addToast: (message: string, type?: ToastType) => void
}

export const ToastContext = createContext<ToastContextValue>({ addToast: () => {} })
