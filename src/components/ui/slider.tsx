import * as React from "react"
import { Slider as SliderPrimitive } from "radix-ui"

import { cn } from "@/lib/utils"

function Slider({
  className,
  defaultValue,
  value,
  min = 0,
  max = 100,
  ...props
}: React.ComponentProps<typeof SliderPrimitive.Root>) {
  const _values = React.useMemo(
    () =>
      Array.isArray(value)
        ? value
        : Array.isArray(defaultValue)
          ? defaultValue
          : [min, max],
    [value, defaultValue, min, max]
  )

  const [isDragging, setIsDragging] = React.useState(false)
  const [isActive, setIsActive] = React.useState(false)
  const timerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null)

  function handlePointerDown(e: React.PointerEvent) {
    if (timerRef.current) clearTimeout(timerRef.current)
    const isThumb = !!(e.target as Element).closest('[role="slider"]')
    setIsDragging(isThumb)
    setIsActive(true)
  }

  function handlePointerUp() {
    setIsDragging(false)
    timerRef.current = setTimeout(() => setIsActive(false), 220)
  }

  return (
    <SliderPrimitive.Root
      data-slot="slider"
      data-animating={isDragging ? undefined : ""}
      defaultValue={defaultValue}
      value={value}
      min={min}
      max={max}
      onPointerDown={handlePointerDown}
      onPointerUp={handlePointerUp}
      onPointerLeave={handlePointerUp}
      className={cn(
        "relative flex w-full h-5 touch-none items-center select-none data-[disabled]:opacity-50 data-[orientation=vertical]:h-full data-[orientation=vertical]:min-h-44 data-[orientation=vertical]:w-auto data-[orientation=vertical]:flex-col",
        className
      )}
      {...props}
    >
      <SliderPrimitive.Track
        data-slot="slider-track"
        className={cn(
          "cursor-pointer bg-black/[0.1] dark:bg-white/[0.18] relative grow overflow-hidden rounded-full data-[orientation=horizontal]:h-1.5 data-[orientation=horizontal]:w-full data-[orientation=vertical]:h-full data-[orientation=vertical]:w-1.5"
        )}
      >
        <SliderPrimitive.Range
          data-slot="slider-range"
          className={cn(
            "bg-black/[0.6] dark:bg-white/[0.7] absolute data-[orientation=horizontal]:h-full data-[orientation=vertical]:w-full"
          )}
        />
      </SliderPrimitive.Track>
      {Array.from({ length: _values.length }, (_, index) => (
        <SliderPrimitive.Thumb
          data-slot="slider-thumb"
          key={index}
          className={cn(
            "cursor-pointer block size-4 shrink-0 rounded-full border-2 bg-white shadow-sm transition-[color,box-shadow] focus-visible:outline-hidden disabled:pointer-events-none disabled:opacity-50",
            isActive
              ? "border-blue-500 ring-4 ring-blue-500/30"
              : "border-black/40 dark:border-neutral-500 hover:border-blue-500 hover:ring-4 hover:ring-blue-500/30"
          )}
        />
      ))}
    </SliderPrimitive.Root>
  )
}

export { Slider }
