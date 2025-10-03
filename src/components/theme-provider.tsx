import { createContext, useContext, useEffect, useState } from "react"

type Theme = "dark" | "light"

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

export function ThemeProvider({
  children,
  defaultTheme = "light",
  storageKey = "observer-theme",
  ...props
}: ThemeProviderProps) {
  const [theme, setThemeState] = useState<Theme>(() => {
    try {
      const stored = localStorage.getItem(storageKey)
      console.log('Initial theme from localStorage:', stored)
      if (stored === "dark" || stored === "light") {
        return stored as Theme
      }
      // If no valid stored value, set default and save it
      console.log('No valid stored theme, using default:', defaultTheme)
      localStorage.setItem(storageKey, defaultTheme)
      return defaultTheme
    } catch (error) {
      console.error('Error reading theme from localStorage:', error)
      return defaultTheme
    }
  })

  useEffect(() => {
    console.log('Theme changed to:', theme)
    const root = window.document.documentElement
    
    // Remove both classes first
    root.classList.remove("light", "dark")
    
    // Add the current theme class
    root.classList.add(theme)
    
    console.log('Applied class to root:', theme)
    console.log('Root classes:', root.className)
  }, [theme])

  const setTheme = (newTheme: Theme) => {
    console.log('setTheme called with:', newTheme)
    try {
      localStorage.setItem(storageKey, newTheme)
      console.log('Saved to localStorage:', newTheme)
      setThemeState(newTheme)
    } catch (error) {
      console.error("Failed to save theme:", error)
      setThemeState(newTheme)
    }
  }

  const value = {
    theme,
    setTheme,
  }

  return (
    <ThemeProviderContext.Provider {...props} value={value}>
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
