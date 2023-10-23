import os
from datetime import datetime
from typing import Dict
import shlex

import jsonschema
import humanize
from flask import Flask, render_template, request
import flask
from flask_login import (
    LoginManager, login_required, login_user, current_user, logout_user
)
from flask_httpauth import HTTPBasicAuth
from flask_wtf import FlaskForm
from wtforms import (
    StringField, PasswordField, SelectMultipleField, HiddenField
)
from wtforms.validators import DataRequired
from urllib.parse import urlparse, urljoin, quote_plus
from webdav3.client import Client as WebDAVClient, RemoteResourceNotFound, ResponseErrorCode

from permissions_client import PermissionsClient
from smbdrop_client import ShareClient
import permissions_client.schema as proxy

from mgmtinterface_client.schema import (
    CREATE_SHARE_REQUEST_SCHEMA, CreateShareRequest
)


app = Flask(__name__)

app.config.from_object('mgmtinterface.default_config')
app.config.from_envvar('MGMTINTERFACE_CONFIG')
app.config.from_envvar('MGMTINTERFACE_SECRET')

app.jinja_options["autoescape"] = lambda _: True

login_manager = LoginManager()
login_manager.init_app(app)

apiauth = HTTPBasicAuth()

client = PermissionsClient(
    app.config['PERMISSIONS_API']
)
smbclient = ShareClient(
    app.config['SHARE_API']
)


def is_safe_url(target):
    ref_url = urlparse(request.host_url)
    test_url = urlparse(urljoin(request.host_url, target))
    return test_url.scheme in ('http', 'https') and ref_url.netloc == test_url.netloc


@apiauth.verify_password
def verify_password(username, password):
    if not username or not password:
        return None
    auth_result = client.authenticate(username, password)
    if auth_result['errno'] == 0:
        return username


class User:
    def __init__(self, user_id):
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


class LoginForm(FlaskForm):
    username = StringField(
        'Username',
        validators=[DataRequired()],
        render_kw={'autofocus': True}
    )
    password = PasswordField('Password', validators=[DataRequired()])


class AddProjectForm(FlaskForm):
    name = StringField(
        'name', validators=[DataRequired()],
        render_kw={'autofocus': True}
    )
    members = SelectMultipleField(
        'members',
        validators=[]
    )


class ChangeProjectMembersForm(FlaskForm):
    members = SelectMultipleField(
        'members',
        validators=[]
    )


def default_dataset_name():
    now = datetime.now()
    return now.strftime('%Y-%m-%d-')


class CreateDatasetForm(FlaskForm):
    name = StringField(
        'name', validators=[DataRequired()],
        render_kw={'autofocus': True}
    )


class SealDatasetForm(FlaskForm):
    pass


class ShareDatasetForm(FlaskForm):
    op = HiddenField('op', validators=[DataRequired()])


@app.template_filter()
def format_filesize(value):
    return humanize.naturalsize(value, binary=True)


@app.template_filter()
def format_unixtime(value):
    d = datetime.fromtimestamp(value)
    return d.strftime('%Y-%m-%d %H:%M')


@app.template_filter()
def shellescape(filename):
    return shlex.quote(filename)


@login_manager.user_loader
def load_user(user_id):
    return User(user_id=user_id)


@login_manager.unauthorized_handler
def unauthorized_callback():
    flask.flash("Unauthorized, login required.")
    return flask.redirect(flask.url_for('login'))


@app.route("/login/", methods=["GET", "POST"])
def login():
    form = LoginForm()
    if request.method == 'POST' and form.validate():
        auth_result = client.authenticate(form.username.data, form.password.data)
        if auth_result['errno'] == 0:
            login_user(
                User(
                    user_id=form.username.data,
                )
            )
            next_url = flask.request.args.get('next')
            if not is_safe_url(next_url):
                return flask.abort(400)
            return flask.redirect(next_url or flask.url_for('index'))
        else:
            flask.flash(f"Error: {auth_result['message']}")
    return render_template('login.html.j2', form=form)


@app.route('/')
def index():
    return flask.redirect(flask.url_for('project'))


@app.route("/project/", methods=["GET"])
@login_required
def project():
    projects = client.list_projects()
    if projects['errno'] != 0:
        return render_template(
            'error.html.j2',
            result=projects,
        )
    plist = sorted(projects['projects'], key=lambda p: p['mtime'], reverse=True)
    ownprojects = []
    memberprojects = []
    otherprojects = []
    uid = current_user.user_id
    user_info = client.id_user(uid)
    for p in plist:
        if p['owner'] == uid:
            ownprojects.append(p)
        elif uid in p['members']:
            memberprojects.append(p)
        else:
            otherprojects.append(p)

    shares = smbclient.list()
    own_shareinfo = []
    for _, share in shares.items():
        if share['uid'] == user_info['id']['uid']:
            project, dsname = split_smbpath(share['dest_path'])
            own_shareinfo.append(
                {
                    'project': project,
                    'dsname': dsname,
                }
            )

    # contains: project list, actions: create new project, links to project details
    createprojectform = AddProjectForm()
    return render_template(
        'project.html.j2',
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
    if allusers['errno'] != 0:
        return render_template(
            'error.html.j2',
            result=allusers,
        )
    form.members.choices = [
        (name, name) for name in allusers['users']
    ]
    if request.method == 'POST' and form.validate():
        form_set = set(form.members.data)
        invalids = form_set - set(allusers['users'])
        if invalids:
            # Users that are not known users of the data management
            flask.flash("Bad request, invalid options")
            return render_template(
                'error.html.j2',
                result=allusers,
            )
        result = client.create_project(
            name=form.name.data,
            owner=current_user.user_id,
            members=form.members.data
        )
        if result['errno'] == 0:
            flask.flash(f"Project {form.name.data} created.")
            return flask.redirect(flask.url_for('projectdetails', name=form.name.data))
        else:
            flask.flash(f"Error {result['message']}")
    return render_template(
        'addproject.html.j2',
        form=form,
    )


@app.template_filter()
def nextcloudlink(path, isdir=False):
    if not isdir:
        path = os.path.dirname(path)
    return app.config['NEXTCLOUD_BASE'] + quote_plus(path)


@app.template_filter()
def jupyterlink(path, isdir=False):
    if not isdir:
        path = os.path.dirname(path)
    return app.config['JUPYTER_BASE'] + quote_plus(path)


@app.template_filter()
def libertemlink(path):
    return app.config['LIBERTEM_BASE'] + quote_plus(path)


@app.template_filter()
def rsync(path, isdir=False):
    target = app.config['MOELLENSTEDT_BASE'] + path
    if isdir:
        target += '/'
        src = '<src_path>/'
    else:
        src = '<src_file>'
    return (
        f'rsync -av --info=progress2 -e "ssh -T -c aes128-ctr -o Compression=no -x" "{src}" '
        f'{ shellescape(current_user.user_id + "@" + target) }'
    )


@app.template_filter()
def sftp(path, isdir=False):
    if not isdir:
        path = os.path.dirname(path)
    target = app.config['MOELLENSTEDT_BASE'] + path
    return f'sftp -c aes128-ctr -o Compression=no { shellescape(current_user.user_id + "@" + target) }'


@app.template_filter()
def sampledbsearch(path):
    base = app.config['SAMPLEDB_BASE'] + 'objects/'
    return f'{base}?q="{quote_plus(path)}"+in+file_name&advanced=on'


@app.template_filter()
def main_smbpath(path):
    win_path = path.replace('/', '\\')
    base = app.config['MAIN_SMB_BASE']
    return base + win_path


def smbpath(project, dsname):
    return os.path.join(
        app.config['SMB_BASE'],
        project['relpath'],
        dsname
    )


def split_smbpath(dest_path):
    head, dsname = os.path.split(dest_path)
    _, project = os.path.split(head)
    return (project, dsname)


@app.template_filter()
def canonical_path(project, dsname=None):
    if dsname is None:
        return os.path.join('/', project['relpath'])
    else:
        return os.path.join('/', project['relpath'], dsname)


def add_share_info(datasets, project, uid, shares):
    for ds in datasets:
        dest_path = os.path.normpath(smbpath(project, ds['name']))
        ds['shareinfo'] = None
        ds['shared'] = False
        for key, value in shares.items():
            path_match = os.path.normpath(value['dest_path']) == dest_path
            ds['shared'] = ds['shared'] or path_match
            if path_match and value['uid'] == uid:
                ds['shareinfo'] = value
                ds['shareinfo']['smb_host'] = "moellenstedt.iff.kfa-juelich.de"
                ds['shareinfo']['workgroup'] = "ER-C-INGEST"
    return datasets


@app.route("/project/<name>/details/", methods=["GET"])
@login_required
def projectdetails(name: str):
    pr_info = client.projectdetails(name, fast=True)
    if pr_info['errno'] == 0:
        user_info = client.id_user(current_user.user_id)
        shares = smbclient.list()
        datasets = add_share_info(
            pr_info['datasets'],
            pr_info['project'],
            user_info['id']['uid'],
            shares
        )
        datasets = sorted(datasets, key=lambda ds:ds['mtime'], reverse=True)
        return render_template(
            'projectdetails.html.j2',
            project=pr_info['project'],
            datasets=datasets,
        )
    else:
        return render_template(
            'error.html.j2',
            result=pr_info
        )


@app.route("/project/<name>/fragments/disk_usage/", methods=["GET"])
@login_required
def project_size(name: str):
    """
    Extracted for async access, to not block loading the whole page on this
    potentially slow operation.
    """
    pr_info = client.projectdetails(name, fast=False)
    if pr_info['errno'] == 0:
        return render_template(
            'projectdetails_size.html.j2',
            project=pr_info['project'],
        )
    else:
        return render_template(
            'error.html.j2',
            result=pr_info
        )


@app.route("/project/<name>/members/", methods=["GET", "POST"])
@login_required
def project_members(name: str):
    def error(info):
        # FIXME a 403, 404 or other error codes
        # may be appropriate as well -- to be refined
        return render_template(
            'error.html.j2',
            result=info
        )
    is_error = False
    # First perform member action
    form = ChangeProjectMembersForm()
    pr_info = client.projectdetails(name)

    if pr_info['errno'] == 0:
        allusers = client.list_users()
        if allusers['errno'] != 0:
            return error(allusers)
        form.members.choices = [
            (name, name) for name in allusers['users']
        ]
        updated = False
        if request.method == 'POST' and form.validate():
            updated = True
            form_set = set(form.members.data)
            project_set = set(pr_info['project']['members'])
            invalids = form_set - set(allusers['users'])
            if invalids:
                # Users that are not known users of the data management
                flask.flash("Bad request, invalid options")
                is_error = True
            to_add = form_set - project_set
            to_remove = project_set - form_set
            if to_add and not is_error:
                result = client.change_project_members(
                    projectname=name,
                    op='add',
                    members=list(to_add),
                    user=current_user.user_id,
                )
                if result['errno'] != 0:
                    flask.flash(f"Error adding members {to_add}: {result['message']}.")
                    is_error = True
            if to_remove and not is_error:
                result = client.change_project_members(
                    projectname=name,
                    op='remove',
                    members=list(to_remove),
                    user=current_user.user_id,
                )
                if result['errno'] != 0:
                    flask.flash(f"Error removing members {to_remove}: {result['message']}.")
                    is_error = True
            if not is_error:
                if to_add or to_remove:
                    flask.flash("Project members updated.")
                return flask.redirect(flask.url_for('projectdetails', name=name))
        if updated:
            pr_info = client.projectdetails(name)
        if pr_info['errno'] == 0:
            form.members.data = pr_info['project']['members']
            return render_template(
                'projectmembers.html.j2',
                form=form,
                project=pr_info['project']
            )
    return(error(pr_info))


@app.route("/project/<name>/createdataset/", methods=["GET", "POST"])
@login_required
def create_dataset(name: str):
    # First perform member action
    form = CreateDatasetForm()
    if request.method == 'POST' and form.validate():
        dsname = form.name.data.strip()
        result = client.create_dataset(
            projectname=name,
            name=dsname,
            user=current_user.user_id,
        )
        if result['errno'] == 0:
            flask.flash(f'Session "{dsname}" created in project "{name}".')
            return flask.redirect(flask.url_for('projectdetails', name=name))
        else:
            flask.flash(f"Error: {result['message']}.")
    else:
        form.name.data = default_dataset_name()
    pr_info = client.projectdetails(name)
    if pr_info['errno'] == 0:
        return render_template(
            'createdataset.html.j2',
            project=pr_info['project'],
            datasets=pr_info['datasets'],
            form=form
        )
    else:
        # FIXME a 403, 404 or other error codes
        # may be appropriate as well -- to be refined
        return render_template(
            'error.html.j2',
            result=pr_info
        )


@app.route("/project/<name>/seal/<dataset>/", methods=["GET", "POST"])
@login_required
def seal_dataset(name: str, dataset: str):
    # First perform member action
    form = SealDatasetForm()
    if request.method == 'POST' and form.validate():
        result = client.seal_dataset(
            projectname=name,
            name=dataset,
            user=current_user.user_id,
        )
        if result['errno'] == 0:
            flask.flash(f'Session "{dataset}" in project "{name}" sealed.')
            return flask.redirect(flask.url_for('projectdetails', name=name))
        else:
            flask.flash(f"Error: {result['message']}.")

    pr_info = client.projectdetails(name)
    if pr_info['errno'] == 0:
        return render_template(
            'sealdataset.html.j2',
            project=pr_info['project'],
            datasets=pr_info['datasets'],
            form=form,
            dsname=dataset,
        )
    else:
        # FIXME a 403, 404 or other error codes
        # may be appropriate as well -- to be refined
        return render_template(
            'error.html.j2',
            result=pr_info
        )


@app.route("/project/<name>/share/<dataset>/", methods=["GET", "POST"])
@login_required
def share_dataset(name: str, dataset: str):
    # First perform member action
    form = ShareDatasetForm()
    pr_info = client.projectdetails(name)
    user_info = client.id_user(current_user.user_id)
    shares = smbclient.list()
    datasets = add_share_info(
        pr_info['datasets'],
        pr_info['project'],
        user_info['id']['uid'],
        shares
    )
    ds_by_name = {
        ds['name']: ds
        for ds in datasets
    }
    ds = ds_by_name.get(dataset)
    if ds is None:
        flask.abort(404)
    updated = False
    if request.method == 'POST' and form.validate():
        if (current_user.user_id in pr_info['project']['members']
                or current_user.user_id == pr_info['project']['owner']):
            if form.op.data == 'create':
                updated = True
                try:
                    result = smbclient.create(
                        dest_path=smbpath(pr_info['project'], ds['name']),
                        uid=user_info['id']['uid'],
                        gid=pr_info['project']['gid'],
                    )
                except RuntimeError as e:
                    flask.flash(f"Error: {e}.")
                    return flask.redirect(flask.url_for('share_dataset', name=name, dataset=dataset))
                if result['share_id']:
                    flask.flash(f'Share for session "{dataset}" in project "{name}" created.')
                    return flask.redirect(flask.url_for('share_dataset', name=name, dataset=dataset))
                else:
                    flask.flash(f"Error: {result}.")
            elif form.op.data == 'delete':
                updated = True
                result = smbclient.delete(
                    share_id=ds['shareinfo']['share_id']
                )
                # TODO error handling
                if result:
                    flask.flash(f'Share for session "{dataset}" in project "{name}" deactivated.')
                    return flask.redirect(flask.url_for('share_dataset', name=name, dataset=dataset))
                else:
                    flask.flash(f"Error: {result}.")
            else:
                flask.flash(f"Error: Invalid opcode.")
        else:
            flask.flash(f"Error: Managing share forbidden.")
    if updated:
        pr_info = client.projectdetails(name)
        user_info = client.id_user(current_user.user_id)
        shares = smbclient.list()
        datasets = add_share_info(
            pr_info['datasets'],
            pr_info['project'],
            user_info['id']['uid'],
            shares
        )
        for ds in datasets:
            if ds['name'] == dataset:
                break
    form.op.data = "delete" if ds['shareinfo'] else "create"
    if pr_info['errno'] == 0:
        return render_template(
            'sharedataset.html.j2',
            project=pr_info['project'],
            datasets=datasets,
            form=form,
            ds=ds,
        )
    else:
        # FIXME a 403, 404 or other error codes
        # may be appropriate as well -- to be refined
        return render_template(
            'error.html.j2',
            result=pr_info
        )


@app.route('/logout')
@login_required
def logout():
    logout_user()
    flask.flash('You were logged out.')
    return flask.redirect(flask.url_for('login'))


@app.route('/api/users/auth/', methods=["POST"])
def api_authenticate_user() -> dict:
    request_data: proxy.AuthenticateUserRequest = request.json  # type:ignore
    try:
        jsonschema.validate(
            schema=proxy.AUTHENTICATE_USER_REQUEST_SCHEMA, instance=request_data
        )
    except jsonschema.exceptions.ValidationError as e:
        raise proxy.InvalidAPIUsage(str(e))
    return client.authenticate(request_data['username'], request_data['password'])


@app.route("/api/users/id/<name>/", methods=["GET"])
@apiauth.login_required
def api_id_user(name) -> Dict:
    return client.id_user(name)


@app.route("/api/users/list/", methods=["GET"])
@apiauth.login_required
def api_get_user_list() -> Dict:
    return client.list_users()


@app.route("/api/project/", methods=["POST"])
@apiauth.login_required
def api_create_project() -> Dict:
    request_data: proxy.CreateProjectRequest = request.json  # type:ignore
    try:
        jsonschema.validate(
            schema=proxy.CREATE_PROJECT_REQUEST_SCHEMA, instance=request_data
        )
        if request_data['owner'] != apiauth.current_user():
            raise proxy.InvalidAPIUsage(
                f'Project owner {request_data["owner"]} '
                f'must equal authenticated user {apiauth.current_user()}.'
            )
        return client.create_project(
            name=request_data['name'],
            owner=request_data['owner'],
            members=request_data['members']
        )
    except jsonschema.exceptions.ValidationError as e:
        raise proxy.InvalidAPIUsage(str(e))  # message can possibly be improved


@app.route("/api/project/", methods=["GET"])
@apiauth.login_required
def api_list_projects() -> Dict:
    return client.list_projects()


@app.route("/api/project/<project_name>/", methods=["GET"])
@apiauth.login_required
def api_project_details(project_name) -> Dict:
    return client.projectdetails(project_name)


@app.route("/api/project/<project_name>/dataset/", methods=["POST"])
@apiauth.login_required
def api_create_dataset(project_name) -> Dict:
    request_data: proxy.CreateDatasetRequest = request.json  # type:ignore
    try:
        jsonschema.validate(
            schema=proxy.CREATE_DATASET_REQUEST_SCHEMA, instance=request_data
        )
        if request_data['owner'] != apiauth.current_user():
            raise proxy.InvalidAPIUsage(
                f'Project owner {request_data["owner"]} '
                f'must equal authenticated user {apiauth.current_user()}.'
            )
        return client.create_dataset(
            projectname=project_name,
            name=request_data["name"],
            user=request_data["owner"]
        )
    except jsonschema.exceptions.ValidationError as e:
        raise proxy.InvalidAPIUsage(str(e))  # message can possibly be improved


@app.route("/api/project/<project_name>/dataset/seal/", methods=["POST"])
@apiauth.login_required
def api_seal_dataset(project_name):
    request_data: proxy.CreateDatasetRequest = request.json  # type:ignore
    try:
        jsonschema.validate(
            schema=proxy.SEAL_DATASET_REQUEST_SCHEMA, instance=request_data
        )
        if request_data['user'] != apiauth.current_user():
            raise proxy.InvalidAPIUsage(
                f'API user {request_data["user"]} '
                f'must equal authenticated user {apiauth.current_user()}.'
            )
        return client.seal_dataset(
            projectname=project_name,
            name=request_data["name"],
            user=request_data["user"]
        )
    except jsonschema.exceptions.ValidationError as e:
        raise proxy.InvalidAPIUsage(str(e))  # message can possibly be improved


@app.route("/api/project/<project_name>/members/", methods=["PATCH"])
@apiauth.login_required
def api_change_project_members(project_name):
    request_data: proxy.ChangeMembersRequest = request.json  # type:ignore
    try:
        jsonschema.validate(
            schema=proxy.CHANGE_PROJECT_MEMBERS_REQUEST_SCHEMA, instance=request_data
        )
        if request_data['user'] != apiauth.current_user():
            raise proxy.InvalidAPIUsage(
                f'API user {request_data["user"]} '
                f'must equal authenticated user {apiauth.current_user()}.'
            )
        return client.change_project_members(
            projectname=project_name,
            op=request_data["op"],
            members=request_data["members"],
            user=request_data['user']
        )
    except jsonschema.exceptions.ValidationError as e:
        raise proxy.InvalidAPIUsage(str(e))  # message can possibly be improved


@app.route("/api/share/", methods=["POST"])
@apiauth.login_required
def api_create_share() -> Dict:
    request_data: CreateShareRequest = request.json  # type:ignore
    jsonschema.validate(schema=CREATE_SHARE_REQUEST_SCHEMA, instance=request_data)

    pr_info = client.projectdetails(request_data["project"])
    user_info = client.id_user(apiauth.current_user())
    if pr_info['errno'] != 0:
        raise RuntimeError(pr_info['message'])
    if user_info['errno'] != 0:
        raise RuntimeError(user_info['message'])
    ds = None
    for ds in pr_info['datasets']:
        if ds['name'] == request_data["dataset"]:
            break
    if ds is None:
        raise RuntimeError(
            f"Dataset {ds.name} not found in project {pr_info['project']['name']}"
        )
    if apiauth.current_user() not in pr_info['project']['members']:
        raise RuntimeError(
            f"API user {apiauth.current_user()} "
            f"not in project members {pr_info['project']['members']}."
        )
    share = smbclient.create(
        dest_path=smbpath(pr_info['project'], ds['name']),
        uid=user_info['id']['uid'],
        gid=pr_info['project']['gid'],
    )
    return dict(share)


@app.route("/api/share/<share_id>/", methods=["DELETE"])
@apiauth.login_required
def api_delete_share(share_id: str) -> Dict:
    shares = smbclient.list()
    share = shares.get(share_id, None)
    if share is None:
        raise RuntimeError(f"Share {share_id} not found.")
    user_info = client.id_user(apiauth.current_user())
    if user_info['errno'] != 0:
        raise RuntimeError(user_info['message'])
    if user_info['id']['uid'] != share['uid']:
        raise RuntimeError(
            f"API user {user_info['id']['uid']} "
            f"does not match share owner {share['uid']}."
        )
    return smbclient.delete(share_id)


@app.route("/api/share/", methods=["GET"])
@apiauth.login_required
def api_list_shares() -> Dict:
    result = {}
    shares = smbclient.list()
    user_info = client.id_user(apiauth.current_user())
    if user_info['errno'] != 0:
        raise RuntimeError(user_info['message'])
    for key, share in shares.items():
        if user_info['id']['uid'] == share['uid']:
            result[key] = share
    return result


def get_path_pyramid(path):
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

    path_pyramid.insert(0, ('/', 'er-c-data'))
    return path_pyramid


def get_first_existing(cl: WebDAVClient, path_pyramid):
    parent = '/'
    for parent, _ in reversed(path_pyramid):
        try:
            if cl.info(parent) and cl.is_dir(parent):
                break
        except (RemoteResourceNotFound, ResponseErrorCode):
            # If the path pyramid contains no valid path,
            # this will be the final value for the next step
            parent = '/'
            continue
    return parent


@app.route("/browse/", methods=["GET"])
@login_required
def browse():
    path = request.args.get('path')
    if path is None:
        return flask.redirect(flask.url_for('browse', path='/'))
    options = {
        'webdav_hostname': app.config['WEBDAV_BASE'],
        'webdav_login': current_user.user_id,
        'webdav_password': '1234',
        'webdav_timeout': 30,
    }
    # We make sure that the path is normalized
    # so that we point with the same path to the same object
    normpath = os.path.normpath(path)
    cl = WebDAVClient(options)
    # This adds a leading slash if it is missing
    resource = cl.resource(normpath)
    # normpath or WebDAV modified the path
    if resource.urn.path() != path:
        return flask.redirect(flask.url_for('browse', path=resource.urn.path()))

    # From here on the path is normalized
    path_pyramid = get_path_pyramid(path)

    try:
        info = resource.info()
        info['isdir'] = resource.is_dir()
        # To be set later if appropriate
        info['project'] = None
    except (RemoteResourceNotFound, ResponseErrorCode) as e:
        if isinstance(e, RemoteResourceNotFound):
            code = 404
            message = f'Remote path {path} not found.'
        else:
            code = e.code
            message = e.message
        result = {
            'errno': code,
            'message': message,
            'path': path,
        }
        parent = get_first_existing(cl, path_pyramid)
        flask.flash(f"Error {code}: {message}")
        return render_template(
            'browse_error.html.j2',
            result=result,
            backto=parent
        )
    members = cl.list(path, get_info=True)
    member_count = len(members)
    if member_count > 1024:
        flask.flash(f"Warning: Only showing 1024 of {member_count} entries.")
        members = members[:1024]
    members.sort(key=lambda m: (not m['isdir'], m['name']), reverse=False)
    # We make sure that the path is normalized
    # so that we point with the same path to the same object
    for m in members:
        m['path'] = os.path.normpath(m['path'])
    info['member_count'] = member_count

    one_up_member = {
        'name': '..',
        'path': None,
        'isdir': True,
        'created': None,
        'size': None,
        'modified': None,
        'etag': None,
        'content_type': None,
    }
    if len(path_pyramid) > 2:
        one_up_member['path'] = path_pyramid[-2][0]
        info['project'] = path_pyramid[2][1]
    elif len(path_pyramid) == 2:
        one_up_member['path'] = '/'
    if one_up_member['path'] is not None:
        members.insert(0, one_up_member)
    components = {
        'info': info,
        'path': path,
        'members':  members,
        'path_pyramid': path_pyramid,
    }
    return render_template(
        'browse.html.j2',
        components=components,
    )
