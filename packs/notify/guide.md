# Sending work out: the "notify" pack

Two commands and one role. An agent chooses **what** is sent, among the files it may read; you,
the operator, choose **where** it goes, once, with the secret. Nothing in a request, a file or a
report can change the recipients.

## Set it up

1. Install the pack (`talixio-agent pack install notify`) and restart the master.
2. On the master, page "Secrets", add what you use:

   | Channel | Command | Secret (the value) | Settings beside it |
   |---|---|---|---|
   | E-mail | `notify/send-mail` | `NOTIFY_SMTP_PASSWORD`: the password of the sending account (an application password where the provider offers one) | `NOTIFY_SMTP_HOST`, `NOTIFY_SMTP_PORT` (465, or 587 for STARTTLS), `NOTIFY_SMTP_USER`, `NOTIFY_MAIL_FROM`, `NOTIFY_MAIL_TO` (one to ten addresses, separated by commas) |
   | Channel | `notify/send-webhook` | `NOTIFY_WEBHOOK_URL`: the address of the incoming webhook | `NOTIFY_WEBHOOK_KIND`: `slack`, `discord`, `teams`, or `json` for any https address that takes `{"title", "text"}` |

3. Choose who may use each secret: the role `notify/sender`, and the master itself if you want
   it to send without creating an agent.

## Use it

Tell the master, in the brief of the work: "when the report is built, send `report.html` by
e-mail and post `summary.md` to the channel". The master runs the commands itself, or creates
an agent with the role `notify/sender` and read access to the files to send.

## What to know

- An agent with these commands can send **any file it may read** to your recipients. Give them
  to agents that read only what those recipients may see.
- Mail goes over an encrypted connection or not at all; a file is always an attachment, never
  the body of the message.
- A webhook takes text, not files: the start of the file is posted (a few thousand characters),
  mentions are neutralised, and the message says when it was cut.
- The commands write to the mail server or to the webhook address you gave, and nowhere else;
  they follow no redirection and use no proxy.
