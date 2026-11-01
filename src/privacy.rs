use crate::config::OperatorConfig;
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::page,
    view::{View, view},
};

#[page("/privacy")]
async fn privacy(cx: &Cx) -> Result<impl View> {
    let operator = app_context::<OperatorConfig>(cx);
    let email = operator.contact_email.as_str();
    let contact_href = operator.contact_href();
    Ok(view! {
        <main class="mx-auto max-w-[824px] px-6 pt-10 pb-16">
            <h1 class="text-3xl font-bold">"Privacy policy"</h1>
            <div
                class="mt-8 flex flex-col gap-6 rounded-[20px] bg-card p-6 leading-relaxed sm:p-10"
            >
                <p>
                    "Flicksy helps users create videos from music, images, and instructions. This policy describes how Flicksy handles information when you use this installation of the service."
                </p>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"Information Flicksy stores"</h2>
                    <p>
                        "Flicksy stores your account username, a salted password hash, and sign-in session records. It uses a session cookie to keep you signed in. Flicksy also stores the project titles, instructions, credits, media files, and rendered videos you provide or create."
                    </p>
                    <p>
                        "The server and its hosting infrastructure may record operational information, such as request timing and error logs, to run and troubleshoot the service."
                    </p>
                </section>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"Video production"</h2>
                    <p>
                        "To create a video, Flicksy sends project information, including titles, credits, instructions, and asset metadata, to the language-model service configured by the operator. Tool results from the production process may also be sent to that service. The service may be hosted by a third party; its handling of this information depends on the operator's configuration and the provider's terms."
                    </p>
                </section>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"YouTube and Google data"</h2>
                    <p>
                        "If you choose to connect YouTube, Google asks for your permission to let Flicksy identify your YouTube channel and upload videos. Flicksy stores the connected channel's ID and title, an encrypted authorization token, and upload records. Upload records include the video details you supply, transfer status, and any YouTube video ID returned. An encrypted temporary upload-session URL may be stored to resume an interrupted upload."
                    </p>
                    <p>
                        "Flicksy sends the video and the title, description, visibility, and audience settings you select to YouTube when you initiate an upload. Google and YouTube handle that information under their own terms and policies."
                    </p>
                    <p>
                        "You can disconnect your channel and delete stored YouTube data in Flicksy's YouTube settings. Disconnecting stops active uploads, immediately deletes stored channel details, upload history, video links, and resumable-upload sessions, and asks Google to revoke authorization. If Google cannot confirm revocation, Flicksy keeps only an encrypted revocation credential and the account identifier needed to retry hourly for up to seven days, then deletes that credential. Reconnecting is blocked while revocation is pending. You can also revoke Flicksy's access directly in your "
                        <a
                            class="text-accent-ink underline"
                            href="https://security.google.com/settings/security/permissions"
                            target="_blank"
                            rel="noopener noreferrer"
                        >
                            "Google account permissions"
                        </a>
                        ". Disconnecting does not delete your projects, original media, local renders, or videos already uploaded to YouTube, and cannot undo data already received by YouTube. See the "
                        <a
                            class="text-accent-ink underline"
                            href="https://policies.google.com/privacy"
                            target="_blank"
                            rel="noopener noreferrer"
                        >
                            "Google Privacy Policy"
                        </a>
                        " and "
                        <a
                            class="text-accent-ink underline"
                            href="https://www.youtube.com/t/terms"
                            target="_blank"
                            rel="noopener noreferrer"
                        >
                            "YouTube Terms of Service"
                        </a>
                        "."
                    </p>
                </section>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"Retention and deletion"</h2>
                    <p>
                        "Flicksy keeps account, session, and project information so you can use the service and return to your work. Signing out removes the current session. While the server is running, Flicksy checks YouTube authorization at startup and hourly. Confirmed authorization loss removes the stored connection and all local YouTube upload records. Temporary network or configuration failures are retried rather than treated as revoked consent. Connections that cannot be verified for 30 days are removed, including their local YouTube data. Maintenance resumes on server startup; these checks cannot run while the server is stopped."
                    </p>
                    <p>
                        "To request access to or deletion of your Flicksy account, projects, or stored YouTube information, contact the operator at "
                        <a
                            class="text-accent-ink underline"
                            href=(contact_href.as_str())
                        >
                            (email)
                        </a>
                        ". The operator is responsible for deleting affected backup copies within applicable retention deadlines and preventing deleted data from being restored. Use Disconnect YouTube in account settings to request deletion of local YouTube data without deleting your projects. Manage videos uploaded to YouTube through YouTube Studio."
                    </p>
                </section>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"Contact"</h2>
                    <p>
                        "For privacy questions or requests, email "
                        <a
                            class="text-accent-ink underline"
                            href=(contact_href.as_str())
                        >
                            (email)
                        </a>
                        "."
                    </p>
                </section>
            </div>
        </main>
    })
}
