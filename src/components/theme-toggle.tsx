import { Moon, Sun, Monitor } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useTheme } from "@/components/theme-provider"

export function ThemeToggle() {
  const { theme, setTheme } = useTheme()

  const cycle = () => {
    if (theme === "light") setTheme("dark")
    else if (theme === "dark") setTheme("system")
    else setTheme("light")
  }

  const icon = theme === "dark" ? <Moon className="h-5 w-5" /> : theme === "system" ? <Monitor className="h-5 w-5" /> : <Sun className="h-5 w-5" />
  const label = theme === "light" ? "Switch to dark" : theme === "dark" ? "Switch to system" : "Switch to light"

  return (
    <Button
      variant="ghost"
      size="icon"
      onClick={cycle}
      className="text-foreground hover:bg-accent border border-border"
      title={label}
    >
      {icon}
      <span className="sr-only">{label}</span>
    </Button>
  )
}
