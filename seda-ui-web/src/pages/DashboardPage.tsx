import { BundleDetailsPage } from './BundleDetailsPage'
import { RepeatedTasksPage } from './RepeatedTasksPage'
import { SessionPage } from './SessionPage'

export function DashboardPage() {
  return (
    <div className="space-y-10">
      <section className="scroll-mt-6">
        <SessionPage />
      </section>

      <div className="border-t border-[rgb(var(--border))]" />

      <section className="scroll-mt-6">
        <RepeatedTasksPage />
      </section>

      <div className="border-t border-[rgb(var(--border))]" />

      <section className="scroll-mt-6">
        <BundleDetailsPage />
      </section>
    </div>
  )
}

