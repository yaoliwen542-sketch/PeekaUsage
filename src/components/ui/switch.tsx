import * as React from "react";
import * as SwitchPrimitive from "@radix-ui/react-switch";

import { cn } from "@/lib/utils";

function Switch({
  className,
  ...props
}: React.ComponentProps<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      className={cn(
        // 紧凑尺寸：18px 高，未选中走 ghost 底 + 细边，选中走主色，全设置页统一
        "peer inline-flex h-[18px] w-8 shrink-0 items-center rounded-full border border-border bg-ghost shadow-none transition-colors duration-150 outline-none",
        "data-[state=checked]:border-transparent data-[state=checked]:bg-primary",
        "focus-visible:ring-2 focus-visible:ring-primary/50",
        "disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className={cn(
          "pointer-events-none block size-3.5 rounded-full bg-text-secondary shadow-sm ring-0 transition-transform duration-150",
          "data-[state=unchecked]:translate-x-[1px]",
          "data-[state=checked]:translate-x-[15px] data-[state=checked]:bg-white",
        )}
      />
    </SwitchPrimitive.Root>
  );
}

export { Switch };
