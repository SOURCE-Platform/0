import { createContext, useContext, useState } from "react"

type UIPrefs = {
  showDescriptions: boolean
  setShowDescriptions: (v: boolean) => void
}

const UIPrefsContext = createContext<UIPrefs>({
  showDescriptions: true,
  setShowDescriptions: () => null,
})

export function UIPrefsProvider({ children }: { children: React.ReactNode }) {
  const [showDescriptions, setShowDescriptionsState] = useState<boolean>(() => {
    try {
      const stored = localStorage.getItem("ui-show-descriptions")
      return stored !== "false"
    } catch {
      return true
    }
  })

  const setShowDescriptions = (v: boolean) => {
    try {
      localStorage.setItem("ui-show-descriptions", String(v))
    } catch {
      // ignore
    }
    setShowDescriptionsState(v)
  }

  return (
    <UIPrefsContext.Provider value={{ showDescriptions, setShowDescriptions }}>
      {children}
    </UIPrefsContext.Provider>
  )
}

export const useUIPrefs = () => useContext(UIPrefsContext)
