import type { ButtonHTMLAttributes } from "react";

export function Button({ style, ...props }: ButtonHTMLAttributes<HTMLButtonElement>) {
	return (
		<button
			type="button"
			style={{
				background: "var(--ns-color-accent)",
				color: "#ffffff",
				border: "none",
				borderRadius: "var(--ns-radius)",
				padding: "8px 16px",
				font: "inherit",
				cursor: "pointer",
				...style,
			}}
			{...props}
		/>
	);
}
