import { Fragment, type ReactNode } from 'react'
import { Link } from 'react-router-dom'

export interface BreadcrumbItem {
  label: string
  /** Omit `href` on the last (current) item — it renders as plain text. */
  href?: string
}

interface PageHeaderProps {
  breadcrumb: BreadcrumbItem[]
  title: string
  status?: ReactNode
  metadata?: ReactNode[]
}

export function PageHeader({ breadcrumb, title, status, metadata }: PageHeaderProps) {
  return (
    <div className="mb-6 pb-5 border-b border-gray-200">
      {breadcrumb.length > 0 && (
        <nav
          aria-label="Breadcrumb"
          className="text-sm text-gray-500 mb-3 flex items-center gap-1.5 flex-wrap"
        >
          {breadcrumb.map((item, i) => {
            const isLast = i === breadcrumb.length - 1
            return (
              <Fragment key={`${item.label}-${i}`}>
                {i > 0 && (
                  <span aria-hidden className="text-gray-300">
                    ›
                  </span>
                )}
                {item.href && !isLast ? (
                  <Link to={item.href} className="hover:text-gray-700 hover:underline">
                    {item.label}
                  </Link>
                ) : (
                  <span className={isLast ? 'text-gray-700' : ''}>{item.label}</span>
                )}
              </Fragment>
            )
          })}
        </nav>
      )}
      <div className="flex items-baseline justify-between gap-3 flex-wrap">
        <h1 className="text-2xl font-semibold text-gray-900">{title}</h1>
        {status}
      </div>
      {metadata && metadata.length > 0 && (
        <div className="mt-1 flex items-center gap-2 text-sm text-gray-500 flex-wrap">
          {metadata.map((item, i) => (
            <Fragment key={i}>
              {i > 0 && (
                <span aria-hidden className="text-gray-300">
                  ·
                </span>
              )}
              {item}
            </Fragment>
          ))}
        </div>
      )}
    </div>
  )
}
