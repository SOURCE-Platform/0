import { Moon, Sun } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useTheme } from "@/components/theme-provider"

export function ThemeToggle() {
  const { theme, setTheme } = useTheme()

  const handleToggle = () => {
    const newTheme = theme === "light" ? "dark" : "light"
    console.log('Current theme:', theme)
    console.log('Switching to:', newTheme)
    setTheme(newTheme)
  }

  console.log('ThemeToggle render - current theme:', theme)

  return (
    <Button
      variant="ghost"
      size="icon"
      onClick={handleToggle}
      className="text-foreground hover:bg-accent border border-border"
      title={'Switch to ' + (theme === "light" ? "dark" : "light") + ' mode'}
    >
      {theme === "dark" ? (
        <Sun className="h-5 w-5" />
      ) : (
        <Moon className="h-5 w-5" />
      )}
      <span className="sr-only">Toggle theme</span>
    </Button>
  )
}
