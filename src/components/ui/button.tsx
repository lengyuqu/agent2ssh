import { forwardRef } from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "../../lib/utils";

const buttonVariants = cva(
  "inline-flex appearance-none items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-semibold transition-colors cursor-pointer outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background disabled:pointer-events-none disabled:opacity-55 [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground border border-transparent hover:bg-primary/90",
        secondary:
          "bg-secondary text-secondary-foreground border border-transparent hover:bg-secondary/80",
        destructive:
          "bg-destructive text-destructive-foreground border border-transparent hover:bg-destructive/90",
        outline:
          "border border-input bg-card text-foreground hover:bg-muted hover:text-foreground",
        ghost: "border border-transparent bg-transparent text-foreground hover:bg-muted",
        link: "border-0 bg-transparent text-primary underline-offset-4 hover:underline"
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 rounded-md px-3 text-xs",
        lg: "h-10 rounded-md px-6",
        icon: "size-9"
      },
      /**
       * Fills the container. Buttons are otherwise sized by their label, so
       * this needed naming: five call sites (`ForwardPanel`, `ProxyPanel`,
       * `ExecPanel`, `AddHostForm`, `MultiExecPanel`) already spelled it by
       * hand as `className="w-full"`, and the settings panel adds nineteen
       * more. Those five are left as they are, not converted. See `align` for
       * the other half of the row shape.
       */
      block: { true: "w-full" },
      /**
       * Content alignment. `start` is a row -- a settings row, a menu item or
       * the settings-panel trigger -- rather than a button. It carries
       * `text-left` because the app imports Tailwind's theme and utilities
       * layers but not `preflight`, so a `<button>` keeps the UA's
       * `text-align: center`; without it a truncating label would put its
       * ellipsis in the middle of the row.
       */
      align: { center: "", start: "justify-start text-left" }
    },
    defaultVariants: { variant: "default", size: "default" }
  }
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, block, align, type = "button", ...props }, ref) => (
    <button
      ref={ref}
      type={type}
      className={cn(buttonVariants({ variant, size, block, align }), className)}
      {...props}
    />
  )
);
Button.displayName = "Button";

