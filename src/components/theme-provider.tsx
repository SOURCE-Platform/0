import { createContext, useContext, useEffect, useState } from "react"
import { getCurrentWindow } from "@tauri-apps/api/window"

type Theme = "dark" | "light" | "system"

type ThemeProviderProps = {
  children: React.ReactNode
  defaultTheme?: Theme
  storageKey?: string
}

type ThemeProviderState = {
  theme: Theme
  setTheme: (theme: Theme) => void
}

const initialState: ThemeProviderState = {
  theme: "light",
  setTheme: () => null,
}

const ThemeProviderContext = createContext<ThemeProviderState>(initialState)

function applyTheme(root: HTMLElement, resolvedTheme: "dark" | "light") {
  root.classList.remove("light", "dark")
  root.classList.add(resolvedTheme)
  root.style.colorScheme = resolvedTheme
  void getCurrentWindow().setTheme(resolvedTheme).catch(() => {
    // Browser previews do not expose a native Tauri window.
  })
}

export function ThemeProvider({
  children,
  defaultTheme = "light",
  storageKey = "observer-theme",
  ...props
}: ThemeProviderProps) {
  const [theme, setThemeState] = useState<Theme>(() => {
    try {
      const stored = localStorage.getItem(storageKey)
      if (stored === "dark" || stored === "light" || stored === "system") {
        return stored as Theme
      }
      localStorage.setItem(storageKey, defaultTheme)
      return defaultTheme
    } catch {
      return defaultTheme
    }
  })

  useEffect(() => {
    const root = window.document.documentElement

    if (theme === "system") {
      const systemTheme = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"
      applyTheme(root, systemTheme)

      const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)")
      const handler = (e: MediaQueryListEvent) => {
        applyTheme(root, e.matches ? "dark" : "light")
      }
      mediaQuery.addEventListener("change", handler)
      return () => mediaQuery.removeEventListener("change", handler)
    } else {
      applyTheme(root, theme)
    }
  }, [theme])

  const setTheme = (newTheme: Theme) => {
    try {
      localStorage.setItem(storageKey, newTheme)
    } catch {
      // ignore
    }
    setThemeState(newTheme)
  }

  return (
    <ThemeProviderContext.Provider {...props} value={{ theme, setTheme }}>
      {children}
    </ThemeProviderContext.Provider>
  )
}

export const useTheme = () => {
  const context = useContext(ThemeProviderContext)
  if (context === undefined)
    throw new Error("useTheme must be used within a ThemeProvider")
  return context
}
