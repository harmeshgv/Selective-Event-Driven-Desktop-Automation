# FlowPilot Desktop Assistant (MVP Skeleton)

This repository is a monorepo for a production-grade desktop assistant that:

1. Observes user activity (mouse, keyboard, active window, periodic screenshots)
2. Converts actions into structured logs
3. Ingests logs via a backend API
4. Builds toward workflow understanding, automation planning, and safe execution

## Architecture (high level)

- `app/observer`: Python service that captures user activity and emits structured JSON logs
- `app/backend`: FastAPI service that receives logs and persists them (Postgres)
- `app/ai`: LLM/embeddings abstraction (stubbed for MVP)
- `app/automation`: execution engine (step-based; Playwright/pyautogui for MVP)
- `app/frontend`: Tauri + React UI (scaffolded; functional MVP coming next)

## Quick Start (MVP loop: observer -> backend)

1. Start Postgres (optional; backend can also run with SQLite by changing `DATABASE_URL`)
   - `docker compose up -d`
2. Configure environment
   - Copy `.env.example` to `.env` (then adjust as needed)
3. Install observer deps and run
   - `cd app/observer`
   - `pip install -r requirements.txt`
   - `python main.py`
   - If your laptop feels laggy while observing, start with lighter settings in `.env`:
     - `MOUSE_MOVE_HZ=0` to disable move tracking
     - `SCREENSHOTS_ENABLED=false` to avoid periodic full-screen captures
     - `CAPTURE_WINDOW_TITLE_KEYWORDS=Chrome,Code` to limit capture to specific apps
4. Install backend deps and run
   - `cd app/backend`
   - `pip install -r requirements.txt`
   - Install automation deps too (required for `/run`):
     - `pip install -r ../automation/requirements.txt`
     - For Playwright, also install browsers:
       - `python -m playwright install`
   - `uvicorn main:app --reload --port 8000`

Open the backend logs; you should see `/logs` receiving events from the observer.

## MVP Automation Test (Safety Preview -> Approval)

On backend startup, the server seeds a sample automation (demo) so you can validate safety UX immediately.

1. Fetch automations:
   - `curl "http://localhost:8000/automations?limit=10"`
2. Preview an automation:
   - Use the `automation_id` you see in step 1:
     - `curl -X POST "http://localhost:8000/run" -H "Content-Type: application/json" -d "{\"automation_id\":<AUTOMATION_ID>,\"preview\":true,\"approved\":false}"`
3. Execute only after approval (blocked if not approved):
   - Preview already shows the step-by-step plan.
   - Then call `/run` with `preview:false` and `approved:true`:
     - `curl -X POST "http://localhost:8000/run" -H "Content-Type: application/json" -d "{\"automation_id\":<AUTOMATION_ID>,\"preview\":false,\"approved\":true}"`

## Next steps (not implemented yet)

- Task grouping, pattern detection, and automation suggestion
- LLM-based task naming + plan generation
- Frontend pages: dashboard, timeline, automation preview/run, settings

## Step Editing (Edit -> Re-Preview)

You can edit the step definitions before executing:

1. Fetch an automation:
   - `curl "http://localhost:8000/automations?limit=10"`
2. Update its steps (all step fields required by this MVP endpoint):
   - `PUT http://localhost:8000/automations/<AUTOMATION_ID>/steps`

