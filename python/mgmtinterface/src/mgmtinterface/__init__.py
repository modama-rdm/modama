import os
from datetime import datetime
import shlex
from typing import TypedDict, final
from typing_extensions import NotRequired

import jsonschema
import humanize
from flask import Flask, render_template, request
from smbdrop_client.schema import (
    DeleteShareResponse,
    ListSharesResponse,
    SerializedShare,
)
from werkzeug.middleware.proxy_fix import ProxyFix
import flask
from flask_login import (
    LoginManager,
    login_required,
    login_user,
    current_user,
    logout_user,
)
from flask_httpauth import HTTPBasicAuth
from flask_wtf import FlaskForm
from wtforms import StringField, PasswordField, SelectMultipleField, HiddenField
from wtforms.validators import DataRequired
from urllib.parse import urlparse, urljoin, quote_plus, quote, urlencode
from webdav3.client import (
    Client as WebDAVClient,
    RemoteResourceNotFound,
    ResponseErrorCode,
)

from permissions_client import PermissionsClient
from smbdrop_client import ShareClient
import permissions_client.schema as proxy

from mgmtinterface_client.schema import CREATE_SHARE_REQUEST_SCHEMA, CreateShareRequest
from .token_client import TokenClient

app = Flask(__name__)
app.wsgi_app = ProxyFix(app.wsgi_app, x_for=1, x_proto=1, x_port=1)

app.config.from_object("mgmtinterface.default_config")
app.config.from_envvar("MGMTINTERFACE_CONFIG")
app.config.from_envvar("MGMTINTERFACE_SECRET")

app.jinja_options["autoescape"] = lambda _: True

login_manager = LoginManager()
login_manager.init_app(app)

apiauth = HTTPBasicAuth()

client = PermissionsClient(app.config["PERMISSIONS_API"])
smbclient = ShareClient(app.config["SHARE_API"])
token_client = TokenClient(
    app.config["TOKEN_SERVICE_URL"],
    ca_cert=app.config["CA_CERT_PATH"],
)


def is_safe_url(target: str) -> bool:
    ref_url = urlparse(request.host_url)
    test_url = urlparse(urljoin(request.host_url, target))
    return test_url.scheme in ("http", "https") and ref_url.netloc == test_url.netloc


@apiauth.verify_password
def verify_password(username: str, password: str):
    if not username or not password:
        return None
    auth_result = client.authenticate(username, password)
    if auth_result["errno"] == 0:
        return username


@final
class User:
    def __init__(self, user_id: str):
        self.user_id = user_id

    @property
    def is_authenticated(self):
        return True

    @property
    def is_active(self):
        return True

    @property
    def is_anonymous(self):
        return False

    def get_id(self):
        return self.user_id


@final
class LoginForm(FlaskForm):
    username = StringField(
        "Username", validators=[DataRequired()], render_kw={"autofocus": True}
    )
    password = PasswordField("Password", validators=[DataRequired()])


@final
class AddProjectForm(FlaskForm):
    name = StringField(
        "name", validators=[DataRequired()], render_kw={"autofocus": True}
    )
    members = SelectMultipleField("members", validators=[])


@final
class ChangeProjectMembersForm(FlaskForm):
    members = SelectMultipleField("members", validators=[])


def default_dataset_name():
    now = datetime.now()
    return now.strftime("%Y-%m-%d-")


@final
class CreateDatasetForm(FlaskForm):
    name = StringField(
        "name", validators=[DataRequired()], render_kw={"autofocus": True}
    )


@final
class SealDatasetForm(FlaskForm):
    pass


@final
class ShareDatasetForm(FlaskForm):
    op = HiddenField("op", validators=[DataRequired()])


@app.template_filter()
def format_filesize(value: int):
    return humanize.naturalsize(value, binary=True)


@app.template_filter()
def format_unixtime(value: float):
    d = datetime.fromtimestamp(value)
    return d.strftime("%Y-%m-%d %H:%M")


@app.template_filter()
def shellescape(filename: str):
    return shlex.quote(filename)


@login_manager.user_loader
def load_user(user_id: str):
    return User(user_id=user_id)


@login_manager.unauthorized_handler
def unauthorized_callback():
    flask.flash("Unauthorized, login required.")
    next_url = urlencode({"next": flask.request.url})
    login_url = flask.url_for("login") + f"?{next_url}"
    return flask.redirect(login_url)


@app.route("/login/", methods=["GET", "POST"])
def login():
    form = LoginForm()
    next_url = flask.request.args.get("next")
    if request.method == "POST" and form.validate():
        auth_result = client.authenticate(form.username.data, form.password.data)
        if auth_result["errno"] == 0:
            login_user(
                User(
                    user_id=form.username.data,
                )
            )
            if not is_safe_url(next_url):
                return flask.abort(400)
            return flask.redirect(next_url or flask.url_for("index"))
        else:
            flask.flash(f"Error: {auth_result['message']}")
    return render_template("login.html.j2", form=form, next_url=next_url)


@app.route("/")
def index():
    return flask.redirect(flask.url_for("project"))


@app.route("/project/", methods=["GET"])
@login_required
def project():
    projects = client.list_projects()
    if projects["errno"] != 0:
        return render_template(
            "error.html.j2",
            result=projects,
        )
    plist = sorted(projects["projects"], key=lambda p: p["mtime"], reverse=True)
    ownprojects: list[proxy.SerializedProject] = []
    memberprojects: list[proxy.SerializedProject] = []
    otherprojects: list[proxy.SerializedProject] = []
    uid: str = current_user.user_id
    user_info = client.id_user(uid)
    for p in plist:
        if p["owner"] == uid:
            ownprojects.append(p)
        elif uid in p["members"]:
            memberprojects.append(p)
        else:
            otherprojects.append(p)

    shares = smbclient.list()
    own_shareinfo = []
    for _, share in shares.items():
        if share["uid"] == user_info["id"]["uid"]:
            project, dsname = split_smbpath(share["dest_path"])
            own_shareinfo.append(
                {
                    "project": project,
                    "dsname": dsname,
                }
            )

    # contains: project list, actions: create new project, links to project details
    createprojectform = AddProjectForm()
    return render_template(
        "project.html.j2",
        ownprojects=ownprojects,
        memberprojects=memberprojects,
        otherprojects=otherprojects,
        createprojectform=createprojectform,
        shares=own_shareinfo,
    )


@app.route("/project/add/", methods=["GET", "POST"])
@login_required
def add_project():
    form = AddProjectForm()
    allusers = client.list_users()
    if allusers["errno"] != 0:
        return render_template(
            "error.html.j2",
            result=allusers,
        )
    form.members.choices = [(name, name) for name in allusers["users"]]
    if request.method == "POST" and form.validate():
        form_set = set(form.members.data)
        invalids = form_set - set(allusers["users"])
        if invalids:
            # Users that are not known users of the data management
            flask.flash("Bad request, invalid options")
            return render_template(
                "error.html.j2",
                result=allusers,
            )
        result = client.create_project(
            name=form.name.data, owner=current_user.user_id, members=form.members.data
        )
        if result["errno"] == 0:
            flask.flash(f"Project {form.name.data} created.")
            return flask.redirect(flask.url_for("projectdetails", name=form.name.data))
        else:
            flask.flash(f"Error {result['message']}")
    return render_template(
        "addproject.html.j2",
        form=form,
    )


@app.template_filter()
def nextcloudlink(path: str, isdir: bool = False) -> str:
    if not isdir:
        path = os.path.dirname(path)
    return app.config["NEXTCLOUD_BASE"] + quote_plus(path)


@app.template_filter()
def jupyterlink(path: str, isdir: bool = False) -> str:
    if not isdir:
        path = os.path.dirname(path)
    return app.config["JUPYTER_BASE"] + quote(path)


@app.template_filter()
def libertemlink(path: str) -> str:
    return app.config["LIBERTEM_BASE"] + quote_plus(path)


@app.template_filter()
def imageviewerlink(path: str) -> str:
    return app.config["IMAGE_VIEWER_BASE"] + quote_plus(path)


def rsync_command(path: str, isdir: bool, user: str) -> str:
    target = app.config["MOELLENSTEDT_BASE"] + path
    if isdir:
        target += "/"
        src = "<src_path>/"
    else:
        src = "<src_file>"
    return (
        f'rsync -av --info=progress2 -e "ssh -T -c aes128-ctr -o Compression=no -x" "{src}" '
        f"{shellescape(user + '@' + target)}"
    )


@app.template_filter()
def rsync(path: str, isdir: bool = False) -> str:
    return rsync_command(path, isdir, current_user.user_id)


def sftp_command(path: str, isdir: bool, user: str) -> str:
    if not isdir:
        path = os.path.dirname(path)
    target = app.config["MOELLENSTEDT_BASE"] + path
    return f"sftp -c aes128-ctr -o Compression=no {shellescape(user + '@' + target)}"


@app.template_filter()
def sftp(path: str, isdir: bool = False) -> str:
    return sftp_command(path, isdir, current_user.user_id)


def quote_path(path: str):
    """
    Consistently quote the path such that it can appear in the
    query part of a URL, as close to the standard as possible
    (https://url.spec.whatwg.org/)
    """
    return quote_plus(path, safe="!$'()*,/:;?@")


@app.template_filter()
def sampledbsearch(path: str):
    base = app.config["SAMPLEDB_BASE"] + "objects/"
    urlpart = quote_path(path)
    search = f'"{urlpart}" in file_name'
    return f"{base}?q={quote_plus(search)}&advanced=on"


@app.template_filter()
def add_path_to_query(url: str, path: str) -> str:
    """
    Add a query string part to the `url` containing the `path`.
    Assumes that the URL doesn't contain a query path yet
    and will add the whole thing, including the leading '?'.
    Assumes the URL has no fragment part.

    Use this template filter to generate a consistenyly encoded
    URL for external referencing.
    """
    return f"{url}?path={quote_path(path)}"


@app.template_filter()
def main_smbpath(path: str) -> str:
    win_path = path.replace("/", "\\")
    base = app.config["MAIN_SMB_BASE"]
    return base + win_path


@app.template_filter()
def smblink(path: str) -> str:
    return app.config["SMB_URL_BASE"] + quote(path)


def smbpath(project: proxy.SerializedProject, dsname: str) -> str:
    return os.path.join(app.config["SMB_BASE"], project["relpath"], dsname)


def split_smbpath(dest_path: str) -> tuple[str, str]:
    head, dsname = os.path.split(dest_path)
    _, project = os.path.split(head)
    return (project, dsname)


@app.template_filter()
def canonical_path(project: proxy.SerializedProject, dsname: str | None = None):
    if dsname is None:
        return os.path.join("/", project["relpath"])
    else:
        return os.path.join("/", project["relpath"], dsname)


class ShareInfo(TypedDict):
    resource: str | None

    share_id: str
    share_name: str
    dest_path: str
    created_timestamp: int

    # temporary credentials:
    username: str
    password: str

    # mapped to this unix user:
    uid: int
    gid: int

    smb_host: NotRequired[str]
    workgroup: NotRequired[str]


class DatasetWithShareInfo(TypedDict):
    name: str
    immutable: bool
    ctime: float
    mtime: float
    shared: bool
    shareinfo: ShareInfo | None


def add_share_info(
        datasets: list[proxy.SerializedDataset],
        project: proxy.SerializedProject,
        uid: int,
        shares: ListSharesResponse,
) -> list[DatasetWithShareInfo]:
    result: list[DatasetWithShareInfo] = []
    for ds_orig in datasets:
        ds: DatasetWithShareInfo = ds_orig  # pyright:ignore[reportAssignmentType]
        dest_path = os.path.normpath(smbpath(project, ds["name"]))
        ds["shareinfo"] = None
        ds["shared"] = False
        for value in shares.values():
            share_info: ShareInfo = value  # pyright:ignore[reportAssignmentType]
            path_match = os.path.normpath(share_info["dest_path"]) == dest_path
            if path_match and share_info["uid"] == uid:
                ds["shared"] = True
                share_info["smb_host"] = "moellenstedt.iff.kfa-juelich.de"
                share_info["workgroup"] = "ER-C-INGEST"
                ds["shareinfo"] = share_info
        result.append(ds)
    return result


@app.route("/project/<name>/details/", methods=["GET"])
@login_required
def projectdetails(name: str):
    pr_info = client.projectdetails(name, fast=True)
    if pr_info["errno"] == 0:
        user_info = client.id_user(current_user.user_id)
        shares = smbclient.list()
        datasets = add_share_info(
            pr_info["datasets"], pr_info["project"], user_info["id"]["uid"], shares
        )
        datasets = sorted(datasets, key=lambda ds: ds["mtime"], reverse=True)
        return render_template(
            "projectdetails.html.j2",
            project=pr_info["project"],
            datasets=datasets,
        )
    else:
        return render_template("error.html.j2", result=pr_info)


@app.route("/project/<name>/fragments/disk_usage/", methods=["GET"])
@login_required
def project_size(name: str):
    """
    Extracted for async access, to not block loading the whole page on this
    potentially slow operation.
    """
    pr_info = client.projectdetails(name, fast=False)
    if pr_info["errno"] == 0:
        return render_template(
            "projectdetails_size.html.j2",
            project=pr_info["project"],
        )
    else:
        return render_template("error.html.j2", result=pr_info)


@app.route("/project/<name>/members/", methods=["GET", "POST"])
@login_required
def project_members(name: str):
    def error(info):
        # FIXME a 403, 404 or other error codes
        # may be appropriate as well -- to be refined
        return render_template("error.html.j2", result=info)

    is_error = False
    # First perform member action
    form = ChangeProjectMembersForm()
    pr_info = client.projectdetails(name)

    if pr_info["errno"] == 0:
        allusers = client.list_users()
        if allusers["errno"] != 0:
            return error(allusers)
        form.members.choices = [(name, name) for name in allusers["users"]]
        updated = False
        if request.method == "POST" and form.validate():
            updated = True
            form_set = set(form.members.data)
            project_set = set(pr_info["project"]["members"])
            invalids = form_set - set(allusers["users"])
            if invalids:
                # Users that are not known users of the data management
                flask.flash("Bad request, invalid options")
                is_error = True
            to_add = form_set - project_set
            to_remove = project_set - form_set
            if to_add and not is_error:
                result = client.change_project_members(
                    projectname=name,
                    op="add",
                    members=list(to_add),
                    user=current_user.user_id,
                )
                if result["errno"] != 0:
                    flask.flash(f"Error adding members {to_add}: {result['message']}.")
                    is_error = True
            if to_remove and not is_error:
                result = client.change_project_members(
                    projectname=name,
                    op="remove",
                    members=list(to_remove),
                    user=current_user.user_id,
                )
                if result["errno"] != 0:
                    flask.flash(
                        f"Error removing members {to_remove}: {result['message']}."
                    )
                    is_error = True
            if not is_error:
                if to_add or to_remove:
                    flask.flash("Project members updated.")
                return flask.redirect(flask.url_for("projectdetails", name=name))
        if updated:
            pr_info = client.projectdetails(name)
        if pr_info["errno"] == 0:
            form.members.data = pr_info["project"]["members"]
            return render_template(
                "projectmembers.html.j2", form=form, project=pr_info["project"]
            )
    return error(pr_info)


@app.route("/project/<name>/createdataset/", methods=["GET", "POST"])
@login_required
def create_dataset(name: str):
    # First perform member action
    form = CreateDatasetForm()
    if request.method == "POST" and form.validate():
        dsname = form.name.data.strip()
        result = client.create_dataset(
            projectname=name,
            name=dsname,
            user=current_user.user_id,
        )
        if result["errno"] == 0:
            flask.flash(f'Session "{dsname}" created in project "{name}".')
            return flask.redirect(flask.url_for("projectdetails", name=name))
        else:
            flask.flash(f"Error: {result['message']}.")
    else:
        form.name.data = default_dataset_name()
    pr_info = client.projectdetails(name)
    if pr_info["errno"] == 0:
        return render_template(
            "createdataset.html.j2",
            project=pr_info["project"],
            datasets=pr_info["datasets"],
            form=form,
        )
    else:
        # FIXME a 403, 404 or other error codes
        # may be appropriate as well -- to be refined
        return render_template("error.html.j2", result=pr_info)


@app.route("/project/<name>/seal/<dataset>/", methods=["GET", "POST"])
@login_required
def seal_dataset(name: str, dataset: str):
    # First perform member action
    form = SealDatasetForm()
    if request.method == "POST" and form.validate():
        result = client.seal_dataset(
            projectname=name,
            name=dataset,
            user=current_user.user_id,
        )
        if result["errno"] == 0:
            flask.flash(f'Session "{dataset}" in project "{name}" sealed.')
            return flask.redirect(flask.url_for("projectdetails", name=name))
        else:
            flask.flash(f"Error: {result['message']}.")

    pr_info = client.projectdetails(name)
    if pr_info["errno"] == 0:
        return render_template(
            "sealdataset.html.j2",
            project=pr_info["project"],
            datasets=pr_info["datasets"],
            form=form,
            dsname=dataset,
        )
    else:
        # FIXME a 403, 404 or other error codes
        # may be appropriate as well -- to be refined
        return render_template("error.html.j2", result=pr_info)


@app.route("/project/<name>/share/<dataset>/", methods=["GET", "POST"])
@login_required
def share_dataset(name: str, dataset: str):
    # First perform member action
    form = ShareDatasetForm()
    pr_info = client.projectdetails(name)
    user_info = client.id_user(current_user.user_id)
    shares = smbclient.list()
    datasets = add_share_info(
        pr_info["datasets"], pr_info["project"], user_info["id"]["uid"], shares
    )
    ds_by_name = {ds["name"]: ds for ds in datasets}
    ds = ds_by_name.get(dataset)
    if ds is None:
        flask.abort(404)
    updated = False
    if request.method == "POST" and form.validate():
        if (
                current_user.user_id in pr_info["project"]["members"]
                or current_user.user_id == pr_info["project"]["owner"]
        ):
            if form.op.data == "create":
                updated = True
                try:
                    result = smbclient.create(
                        dest_path=smbpath(pr_info["project"], ds["name"]),
                        uid=user_info["id"]["uid"],
                        gid=pr_info["project"]["gid"],
                    )
                    result = result["share"]
                except RuntimeError as e:
                    flask.flash(f"Error: {e}.")
                    return flask.redirect(
                        flask.url_for("share_dataset", name=name, dataset=dataset)
                    )
                if result["share_id"]:
                    flask.flash(
                        f'Share for session "{dataset}" in project "{name}" created.'
                    )
                    return flask.redirect(
                        flask.url_for("share_dataset", name=name, dataset=dataset)
                    )
                else:
                    flask.flash(f"Error: {result}.")
            elif form.op.data == "delete":
                updated = True
                result = smbclient.delete(share_id=ds["shareinfo"]["share_id"])
                # TODO error handling
                if result:
                    flask.flash(
                        f'Share for session "{dataset}" in project "{name}" deactivated.'
                    )
                    return flask.redirect(
                        flask.url_for("share_dataset", name=name, dataset=dataset)
                    )
                else:
                    flask.flash(f"Error: {result}.")
            else:
                flask.flash("Error: Invalid opcode.")
        else:
            flask.flash("Error: Managing share forbidden.")
    if updated:
        pr_info = client.projectdetails(name)
        user_info = client.id_user(current_user.user_id)
        shares = smbclient.list()
        datasets = add_share_info(
            pr_info["datasets"], pr_info["project"], user_info["id"]["uid"], shares
        )
        for ds in datasets:
            if ds["name"] == dataset:
                break
    form.op.data = "delete" if ds["shareinfo"] else "create"
    if pr_info["errno"] == 0:
        return render_template(
            "sharedataset.html.j2",
            project=pr_info["project"],
            datasets=datasets,
            form=form,
            ds=ds,
        )
    else:
        # FIXME a 403, 404 or other error codes
        # may be appropriate as well -- to be refined
        return render_template("error.html.j2", result=pr_info)


@app.route("/logout")
@login_required
def logout():
    logout_user()
    flask.flash("You were logged out.")
    return flask.redirect(flask.url_for("login"))


@app.route("/api/users/auth/", methods=["POST"])
def api_authenticate_user() -> proxy.AuthenticateResponse:
    request_data: proxy.AuthenticateUserRequest = request.json  # pyright: ignore[reportAssignmentType]
    try:
        jsonschema.validate(
            schema=proxy.AUTHENTICATE_USER_REQUEST_SCHEMA, instance=request_data
        )
    except jsonschema.exceptions.ValidationError as e:
        raise proxy.InvalidAPIUsage(str(e))
    return client.authenticate(request_data["username"], request_data["password"])


@app.route("/api/users/id/<name>/", methods=["GET"])
@apiauth.login_required
def api_id_user(name) -> proxy.IdUserResponse:
    return client.id_user(name)


@app.route("/api/users/list/", methods=["GET"])
@apiauth.login_required
def api_get_user_list() -> proxy.GetUserListResponse:
    return client.list_users()


@app.route("/api/project/", methods=["POST"])
@apiauth.login_required
def api_create_project() -> proxy.CreateProjectResponse | proxy.GenericErrorResponse:
    request_data: proxy.CreateProjectRequest = request.json  # pyright: ignore[reportAssignmentType]
    try:
        jsonschema.validate(
            schema=proxy.CREATE_PROJECT_REQUEST_SCHEMA, instance=request_data
        )
        if request_data["owner"] != apiauth.current_user():
            raise proxy.InvalidAPIUsage(
                f"Project owner {request_data['owner']} "
                f"must equal authenticated user {apiauth.current_user()}."
            )
        return client.create_project(
            name=request_data["name"],
            owner=request_data["owner"],
            members=request_data["members"],
        )
    except jsonschema.exceptions.ValidationError as e:
        raise proxy.InvalidAPIUsage(str(e))  # message can possibly be improved


@app.route("/api/project/", methods=["GET"])
@apiauth.login_required
def api_list_projects() -> proxy.ListProjectsResponse:
    return client.list_projects()


@app.route("/api/project/<project_name>/", methods=["GET"])
@apiauth.login_required
def api_project_details(
        project_name: str,
) -> proxy.ProjectDetailsResponse | proxy.GenericErrorResponse:
    return client.projectdetails(project_name)


@app.route("/api/project/<project_name>/dataset/", methods=["POST"])
@apiauth.login_required
def api_create_dataset(
        project_name: str,
) -> proxy.CreateDatasetResponse | proxy.GenericErrorResponse:
    request_data: proxy.CreateDatasetRequest = request.json  # pyright: ignore[reportAssignmentType]
    try:
        jsonschema.validate(
            schema=proxy.CREATE_DATASET_REQUEST_SCHEMA, instance=request_data
        )
        if request_data["owner"] != apiauth.current_user():
            raise proxy.InvalidAPIUsage(
                f"Project owner {request_data['owner']} "
                f"must equal authenticated user {apiauth.current_user()}."
            )
        return client.create_dataset(
            projectname=project_name,
            name=request_data["name"],
            user=request_data["owner"],
        )
    except jsonschema.exceptions.ValidationError as e:
        raise proxy.InvalidAPIUsage(str(e))  # message can possibly be improved


@app.route("/api/project/<project_name>/dataset/seal/", methods=["POST"])
@apiauth.login_required
def api_seal_dataset(project_name: str):
    request_data: proxy.SealDatasetRequest = request.json  # pyright: ignore[reportAssignmentType]
    try:
        jsonschema.validate(
            schema=proxy.SEAL_DATASET_REQUEST_SCHEMA, instance=request_data
        )
        if request_data["user"] != apiauth.current_user():
            raise proxy.InvalidAPIUsage(
                f"API user {request_data['user']} "
                f"must equal authenticated user {apiauth.current_user()}."
            )
        return client.seal_dataset(
            projectname=project_name,
            name=request_data["name"],
            user=request_data["user"],
        )
    except jsonschema.exceptions.ValidationError as e:
        raise proxy.InvalidAPIUsage(str(e))  # message can possibly be improved


@app.route("/api/project/<project_name>/members/", methods=["PATCH"])
@apiauth.login_required
def api_change_project_members(
        project_name: str,
) -> proxy.ChangeProjectMembersResponse | proxy.GenericErrorResponse:
    request_data: proxy.ChangeMembersRequest = request.json  # pyright: ignore[reportAssignmentType]
    try:
        jsonschema.validate(
            schema=proxy.CHANGE_PROJECT_MEMBERS_REQUEST_SCHEMA, instance=request_data
        )
        if request_data["user"] != apiauth.current_user():
            raise proxy.InvalidAPIUsage(
                f"API user {request_data['user']} "
                f"must equal authenticated user {apiauth.current_user()}."
            )
        if request_data["op"] not in ("add", "remove"):
            raise proxy.InvalidAPIUsage("op must be add or remove")
        return client.change_project_members(
            projectname=project_name,
            op=request_data["op"],
            members=request_data["members"],
            user=request_data["user"],
        )
    except jsonschema.exceptions.ValidationError as e:
        raise proxy.InvalidAPIUsage(str(e))  # message can possibly be improved


@app.route("/api/share/", methods=["POST"])
@apiauth.login_required
def api_create_share() -> SerializedShare:
    request_data: CreateShareRequest = request.json  # pyright: ignore[reportAssignmentType]
    jsonschema.validate(schema=CREATE_SHARE_REQUEST_SCHEMA, instance=request_data)

    pr_info = client.projectdetails(request_data["project"])
    user_info = client.id_user(apiauth.current_user())
    if pr_info["errno"] != 0:
        raise RuntimeError(pr_info["message"])
    if user_info["errno"] != 0:
        raise RuntimeError(user_info["message"])

    resource = None
    token = request.headers.get("X-Agent-Token")
    if token:
        agent = token_client.verify_agent(token)
        resource = agent.get_resource_name()
        if resource != request_data["resource"]:
            raise RuntimeError("resource does not match token")

    ds = None
    for ds in pr_info["datasets"]:
        if ds["name"] == request_data["dataset"]:
            break
    if ds is None:
        raise RuntimeError(f"Dataset not found in project {pr_info['project']['name']}")
    if (
            apiauth.current_user() not in pr_info["project"]["members"]
            and apiauth.current_user() != pr_info["project"]["owner"]
    ):
        raise RuntimeError(
            f"API user {apiauth.current_user()} "
            f"not in project members {pr_info['project']['members']} and is not the owner."
        )
    resp = smbclient.create(
        dest_path=smbpath(pr_info["project"], ds["name"]),
        uid=user_info["id"]["uid"],
        gid=pr_info["project"]["gid"],
        resource=resource,
    )
    return resp["share"]


@app.route("/api/share/<share_id>/", methods=["DELETE"])
@apiauth.login_required
def api_delete_share(share_id: str) -> DeleteShareResponse:
    shares = smbclient.list()
    share = shares.get(share_id, None)
    if share is None:
        raise RuntimeError(f"Share {share_id} not found.")
    user_info = client.id_user(apiauth.current_user())
    if user_info["errno"] != 0:
        raise RuntimeError(user_info["message"])
    if user_info["id"]["uid"] != share["uid"]:
        raise RuntimeError(
            f"API user {user_info['id']['uid']} "
            f"does not match share owner {share['uid']}."
        )
    return smbclient.delete(share_id)


@app.route("/api/share/", methods=["GET"])
@apiauth.login_required
def api_list_shares() -> ListSharesResponse:
    result: ListSharesResponse = {}
    shares = smbclient.list()
    user_info = client.id_user(apiauth.current_user())
    if user_info["errno"] != 0:
        raise RuntimeError(user_info["message"])
    for key, share in shares.items():
        if user_info["id"]["uid"] == share["uid"]:
            result[key] = share
    return result


def get_path_pyramid(path: str) -> list[str]:
    path_pyramid = []
    # this strips trailing slashes, among other things
    head = os.path.normpath(path)
    tail = True

    # This will always terminate since either the head will get shorter
    # or the tail is empty. Empty head or empty tail terminate the loop.
    while head and tail:
        head, tail = os.path.split(head)
        if tail:
            path_pyramid.insert(0, (os.path.join(head, tail), tail))

    path_pyramid.insert(0, ("/", "er-c-data"))
    return path_pyramid


def get_first_existing(cl: WebDAVClient, path_pyramid: list[str]):
    parent = "/"
    for parent, _ in reversed(path_pyramid):
        try:
            if cl.info(parent) and cl.is_dir(parent):
                break
        except (RemoteResourceNotFound, ResponseErrorCode):
            # If the path pyramid contains no valid path,
            # this will be the final value for the next step
            parent = "/"
            continue
    return parent


@app.route("/browse/", methods=["GET"])
@login_required
def browse():
    path = request.args.get("path")
    if path is None:
        return flask.redirect(flask.url_for("browse", path="/"))
    options = {
        "webdav_hostname": app.config["WEBDAV_BASE"],
        "webdav_login": current_user.user_id,
        "webdav_password": "1234",
        "webdav_timeout": 30,
    }
    # We make sure that the path is normalized
    # so that we point with the same path to the same object
    normpath = os.path.normpath(path)
    cl = WebDAVClient(options)
    # This adds a leading slash if it is missing
    resource = cl.resource(normpath)
    # normpath or WebDAV modified the path
    if resource.urn.path() != path:
        return flask.redirect(flask.url_for("browse", path=resource.urn.path()))

    # From here on the path is normalized
    path_pyramid = get_path_pyramid(path)

    try:
        info = resource.info()
        info["isdir"] = resource.is_dir()
        # To be set later if appropriate
        info["project"] = None
    except (RemoteResourceNotFound, ResponseErrorCode) as e:
        if isinstance(e, RemoteResourceNotFound):
            code = 404
            message = f"Remote path {path} not found."
        else:
            code = e.code
            message = e.message
        result = {
            "errno": code,
            "message": message,
            "path": path,
        }
        parent = get_first_existing(cl, path_pyramid)
        flask.flash(f"Error {code}: {message}")
        return render_template(
            "browse_error.html.j2", result=result, backto=parent
        ), code
    members = cl.list(path, get_info=True)
    member_count = len(members)
    if member_count > 1024:
        flask.flash(f"Warning: Only showing 1024 of {member_count} entries.")
        members = members[:1024]
    members.sort(key=lambda m: (not m["isdir"], m["name"]), reverse=False)
    # We make sure that the path is normalized
    # so that we point with the same path to the same object
    for m in members:
        m["path"] = os.path.normpath(m["path"])
    info["member_count"] = member_count

    one_up_member = {
        "name": "..",
        "path": None,
        "isdir": True,
        "created": None,
        "size": None,
        "modified": None,
        "etag": None,
        "content_type": None,
    }
    if len(path_pyramid) > 2:
        one_up_member["path"] = path_pyramid[-2][0]
        info["project"] = path_pyramid[2][1]
    elif len(path_pyramid) == 2:
        one_up_member["path"] = "/"
    if one_up_member["path"] is not None:
        members.insert(0, one_up_member)
    components = {
        "info": info,
        "path": path,
        "members": members,
        "path_pyramid": path_pyramid,
    }
    return render_template(
        "browse.html.j2",
        components=components,
    )


def browse_current_access(path: str, isdir: bool, user: str) -> dict:
    """Access links and transfer commands for the resource shown at the top of the
    browse screen. Mirrors the "Access this file or folder" cards and the
    rsync/sftp/SMB fields of ``browse.html.j2``."""
    access = {
        "nextcloud": nextcloudlink(path, isdir),
        "jupyter": jupyterlink(path, isdir),
        "libertem": libertemlink(path),
        "sampledb": sampledbsearch(path),
        "rsync": rsync_command(path, isdir, user),
        "sftp": sftp_command(path, isdir, user),
        "smb_windows": main_smbpath(path),
        "smb_unix": smblink(path),
    }
    if not isdir:
        access["imageviewer"] = imageviewerlink(path)
    return access


def browse_member_access(path: str, isdir: bool) -> dict:
    """Per-row external links for the listing table, mirroring the columns of
    ``browse.html.j2``: Nextcloud/JupyterHub for directories, image viewer for
    files, LiberTEM + SampleDB for both."""
    if isdir:
        access = {
            "nextcloud": nextcloudlink(path, isdir),
            "jupyter": jupyterlink(path, isdir),
        }
    else:
        access = {"imageviewer": imageviewerlink(path)}
    access["libertem"] = libertemlink(path)
    access["sampledb"] = sampledbsearch(path)
    return access


@app.route("/api/browse/", methods=["GET"])
@apiauth.login_required
def api_browse():
    """JSON sibling of :func:`browse` for the React UI. Lists a WebDAV path and
    returns it together with the server-computed access links/commands.
    Until the FastAPI browse endpoint (#172) is ready, this can be used.
    mirrors: func:`browse` behaviour. Logical errors come back as an ``errno`` envelope with HTTP 200
    so the SPA can show a friendly not-found state linking to the nearest
    existing parent, rather than a bare HTTP error."""
    user = apiauth.current_user()
    path = request.args.get("path") or "/"
    options = {
        "webdav_hostname": app.config["WEBDAV_BASE"],
        "webdav_login": user,
        "webdav_password": "1234",
        "webdav_timeout": 30,
    }
    normpath = os.path.normpath(path)
    cl = WebDAVClient(options)
    resource = cl.resource(normpath)
    # Canonical path (leading slash / normalization); the client updates its URL
    # to this value instead of us issuing an HTTP redirect.
    canonical = resource.urn.path()
    path_pyramid = get_path_pyramid(canonical)

    try:
        info = resource.info()
        info["isdir"] = resource.is_dir()
    except (RemoteResourceNotFound, ResponseErrorCode) as e:
        if isinstance(e, RemoteResourceNotFound):
            code = 404
            message = f"Remote path {canonical} not found."
        else:
            code = e.code
            message = e.message
        parent = get_first_existing(cl, path_pyramid)
        return {
            "errno": code,
            "message": message,
            "path": canonical,
            "backto": parent,
        }

    members = cl.list(canonical, get_info=True)
    member_count = len(members)
    truncated = member_count > 1024
    if truncated:
        members = members[:1024]
    members.sort(key=lambda m: (not m["isdir"], m["name"]), reverse=False)
    for m in members:
        m["path"] = os.path.normpath(m["path"])

    project = None
    parent_path = None
    if len(path_pyramid) > 2:
        parent_path = path_pyramid[-2][0]
        project = path_pyramid[2][1]
    elif len(path_pyramid) == 2:
        parent_path = "/"

    entries = []
    if parent_path is not None:
        entries.append(
            {
                "name": "..",
                "path": parent_path,
                "isdir": True,
                "created": None,
                "modified": None,
                "size": None,
                "access": {},
            }
        )
    for m in members:
        entries.append(
            {
                "name": m["name"],
                "path": m["path"],
                "isdir": m["isdir"],
                "created": m.get("created"),
                "modified": m.get("modified"),
                "size": m.get("size"),
                "access": browse_member_access(m["path"], m["isdir"]),
            }
        )

    return {
        "errno": 0,
        "path": canonical,
        "info": {
            "name": info.get("name") or "",
            "isdir": info["isdir"],
            "project": project,
            "created": info.get("created"),
            "modified": info.get("modified"),
            "size": info.get("size"),
            "member_count": member_count,
            "truncated": truncated,
        },
        "access": browse_current_access(canonical, info["isdir"], user),
        "path_pyramid": path_pyramid,
        "members": entries,
    }
