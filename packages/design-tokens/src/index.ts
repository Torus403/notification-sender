// The same tokens as tokens.css, for places that can't read CSS variables (generated images, emails).
export const tokens = {
	color: {
		bg: "#ffffff",
		fg: "#0b0b0f",
		muted: "#5b5b66",
		accent: "#2f6bff",
		border: "#e4e4ea",
	},
	radius: 8,
	spaceUnit: 4,
} as const;

export type Tokens = typeof tokens;
