import { forwardRef } from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "../../lib/utils";

const iconButtonVariants = cva(
  "inline-flex appearance-none items-center justify-center rounded-md border bg-card transition-colors cursor-pointer outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-55 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default: "border-border text-muted-foreground hover:bg-muted hover:text-foreground",
        danger: "border-border text-muted-foreground hover:border-destructive/50 hover:text-destructive",
        ghost: "border-transparent bg-transparent text-muted-foreground hover:bg-muted hover:text-foreground"
      },
      size: { default: "size-8", sm: "size-7" }
    },
    defaultVariants: { variant: "default", size: "default" }
  }
);

export interface IconButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof iconButtonVariants> {}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(
  ({ className, variant, size, type = "button", ...props }, ref) => (
    <button
      ref={ref}
      type={type}
      className={cn(iconButtonVariants({ variant, size }), className)}
      {...props}
    />
  )
);
IconButton.displayName = "IconButton";
