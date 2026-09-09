import asyncio
import html
import os
import smtplib

from email.message import EmailMessage
from email.utils import formataddr


SMTP_HOST = os.getenv("SMTP_HOST", "")
SMTP_PORT = int(
    os.getenv("SMTP_PORT", "587")
)
SMTP_SECURITY = os.getenv(
    "SMTP_SECURITY",
    "starttls",
).lower()

SMTP_USERNAME = os.getenv(
    "SMTP_USERNAME",
    "",
)

SMTP_PASSWORD = os.getenv(
    "SMTP_PASSWORD",
    "",
)

SMTP_FROM_EMAIL = os.getenv(
    "SMTP_FROM_EMAIL",
    SMTP_USERNAME,
)

SMTP_FROM_NAME = os.getenv(
    "SMTP_FROM_NAME",
    "Messenger",
)


class EmailDeliveryError(Exception):
    pass


def _send_email(
    to_email: str,
    subject: str,
    text_content: str,
    html_content: str,
) -> None:
    if not SMTP_HOST:
        raise EmailDeliveryError(
            "SMTP_HOST is not configured"
        )

    if not SMTP_FROM_EMAIL:
        raise EmailDeliveryError(
            "SMTP_FROM_EMAIL is not configured"
        )

    message = EmailMessage()

    message["Subject"] = subject
    message["From"] = formataddr(
        (
            SMTP_FROM_NAME,
            SMTP_FROM_EMAIL,
        )
    )
    message["To"] = to_email

    message.set_content(
        text_content
    )

    message.add_alternative(
        html_content,
        subtype="html",
    )

    try:
        if SMTP_SECURITY == "ssl":
            with smtplib.SMTP_SSL(
                SMTP_HOST,
                SMTP_PORT,
                timeout=15,
            ) as smtp:
                if SMTP_USERNAME:
                    smtp.login(
                        SMTP_USERNAME,
                        SMTP_PASSWORD,
                    )

                smtp.send_message(
                    message
                )

            return

        with smtplib.SMTP(
            SMTP_HOST,
            SMTP_PORT,
            timeout=15,
        ) as smtp:
            smtp.ehlo()

            if SMTP_SECURITY == "starttls":
                smtp.starttls()
                smtp.ehlo()

            if SMTP_USERNAME:
                smtp.login(
                    SMTP_USERNAME,
                    SMTP_PASSWORD,
                )

            smtp.send_message(
                message
            )

    except (
        smtplib.SMTPException,
        OSError,
    ) as exc:
        raise EmailDeliveryError(
            "Could not send email"
        ) from exc


async def send_verification_email(
    to_email: str,
    verification_url: str,
) -> None:
    safe_url = html.escape(
        verification_url,
        quote=True,
    )

    subject = (
        "Verify your Messenger account"
    )

    text_content = f"""Welcome to Messenger.

Verify your email address by opening this link:

{verification_url}

This verification link will expire soon.

If you did not create this account, you can ignore this email.
"""

    html_content = f"""
<!doctype html>
<html lang="en">
    <body>
        <h2>Verify your email</h2>

        <p>
            Welcome to Messenger.
        </p>

        <p>
            Click the button below to verify
            your email address.
        </p>

        <p>
            <a href="{safe_url}">
                Verify email
            </a>
        </p>

        <p>
            This verification link will
            expire soon.
        </p>

        <p>
            If you did not create this
            account, you can ignore this
            email.
        </p>
    </body>
</html>
"""

    await asyncio.to_thread(
        _send_email,
        to_email,
        subject,
        text_content,
        html_content,
    )
