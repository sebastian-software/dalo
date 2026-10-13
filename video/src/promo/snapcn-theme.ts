import type {SnapCnTheme} from '@/lib/snap-cn-ui/theme';

import {colors} from '../brand';

// The dalo.sh palette as a snapcn theme: one provider re-skins every component.
export const daloTheme: Partial<SnapCnTheme> = {
  background: '#ffffff',
  foreground: colors.ink,
  card: '#ffffff',
  cardForeground: colors.ink,
  popover: '#ffffff',
  popoverForeground: colors.ink,
  primary: colors.violet,
  primaryForeground: '#ffffff',
  secondary: colors.surface,
  secondaryForeground: colors.ink,
  muted: colors.surface,
  mutedForeground: colors.muted,
  accent: colors.coral,
  accentForeground: '#ffffff',
  destructive: '#c2361f',
  destructiveForeground: '#ffffff',
  border: colors.line,
  input: colors.line,
  ring: colors.violet,
  radius: 20,
  fontFamily: '"Hanken Grotesk"',
};
