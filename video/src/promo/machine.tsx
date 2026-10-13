import type {CSSProperties, ReactNode} from 'react';

import {colors, fonts} from '../brand';
import {Avatar, Card, personas, Spark} from './ui';

export type Row = {
  name: string;
  color?: string;
  // extra decoration on the right: a stamp, a badge, a ring...
  right?: ReactNode;
  style?: CSSProperties;
  strike?: boolean;
};

export const MachineCard = ({
  who,
  title,
  agent,
  rows,
  width = 520,
  footer,
  style,
}: {
  who: keyof typeof personas;
  title: string;
  agent: string;
  rows: Row[];
  width?: number;
  footer?: ReactNode;
  style?: CSSProperties;
}) => (
  <Card style={{width, padding: 30, ...style}}>
    <div style={{display: 'flex', alignItems: 'center', gap: 18}}>
      <Avatar who={who} size={64} />
      <div>
        <div style={{fontFamily: fonts.sans, fontWeight: 700, fontSize: 30, color: colors.ink, letterSpacing: '-0.02em'}}>{title}</div>
        <div style={{fontFamily: fonts.sans, fontWeight: 500, fontSize: 22, color: colors.muted}}>{agent}</div>
      </div>
    </div>
    <div style={{height: 1, background: colors.line2, margin: '24px -30px 18px'}} />
    <div style={{display: 'flex', flexDirection: 'column', gap: 12}}>
      {rows.map((r, i) => (
        <div
          key={i}
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 12,
            padding: '12px 16px',
            borderRadius: 14,
            background: colors.surface,
            border: `1px solid ${colors.line2}`,
            ...r.style,
          }}
        >
          <div style={{display: 'flex', alignItems: 'center', gap: 12, minWidth: 0}}>
            <Spark size={20} color={r.color ?? colors.coral} />
            <span
              style={{
                fontFamily: fonts.mono,
                fontSize: 22,
                fontWeight: 500,
                color: r.strike ? colors.faint : colors.ink,
                textDecoration: r.strike ? 'line-through' : undefined,
                whiteSpace: 'nowrap',
              }}
            >
              {r.name}
            </span>
          </div>
          {r.right}
        </div>
      ))}
    </div>
    {footer}
  </Card>
);
