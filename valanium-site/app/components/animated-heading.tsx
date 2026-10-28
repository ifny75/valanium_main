import { Fragment, type CSSProperties } from 'react';

export type HeadingLine = { text: string; accent?: boolean };

/** Word wrappers preserve natural wrapping; one accessible label avoids spelling out letters. */
export function AnimatedHeading({ lines, className = '' }: { lines: HeadingLine[]; className?: string }) {
  let letterIndex = 0;
  return <h1 className={`animated-heading ${className}`} aria-label={lines.map(line => line.text).join(' ')}>
    {lines.map((line, lineIndex) => <span key={lineIndex} className={`heading-line${line.accent ? ' heading-accent' : ''}`} aria-hidden="true">
      {line.text.split(' ').map((word, wordIndex) => <Fragment key={wordIndex}>
        {wordIndex > 0 ? ' ' : null}
        <span className="heading-word">{Array.from(word).map((letter, index) => <span key={index}
          className="heading-letter" style={{ '--letter-delay': `${120 + letterIndex++ * 22}ms` } as CSSProperties}>{letter}</span>)}</span>
      </Fragment>)}
    </span>)}
  </h1>;
}

export function ButtonArrow() {
  return <svg className="button-arrow" viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <path d="M5 12h14m-6-6 6 6-6 6" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" />
  </svg>;
}
