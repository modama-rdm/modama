from flask import Flask, render_template, request
import flask
from flask_login import LoginManager, login_required, login_user, current_user, logout_user
from flask_wtf import FlaskForm
from wtforms import Field, StringField, PasswordField, Label, HiddenField
from wtforms.widgets import TextInput
from wtforms.validators import DataRequired
from urllib.parse import urlparse, urljoin

from .permissionsclient import PermissionsClient


def is_safe_url(target):
    ref_url = urlparse(request.host_url)
    test_url = urlparse(urljoin(request.host_url, target))
    return test_url.scheme in ('http', 'https') and ref_url.netloc == test_url.netloc


app = Flask(__name__)
login_manager = LoginManager()
login_manager.init_app(app)

app.secret_key = b'caHudnec[drav2'

client = PermissionsClient("http://localhost:5050")


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
    username = StringField('username', validators=[DataRequired()])
    password = PasswordField('Password', validators=[DataRequired()])
    message = Label('message', '')


class CommaListField(Field):
    widget = TextInput()

    def _value(self):
        if self.data:
            return u', '.join(self.data)
        else:
            return u''

    def process_formdata(self, valuelist):
        if valuelist and valuelist[0]:
            self.data = [x.strip() for x in valuelist[0].split(',')]
        else:
            self.data = []  


class AddProjectForm(FlaskForm):
    name = StringField('name', validators=[DataRequired()])
    members = CommaListField('members', validators=[])
    message = Label('message', '')


class AddProjectMembersForm(FlaskForm):
    action = HiddenField('action', default='add')
    members = CommaListField('members', validators=[DataRequired()])
    message = Label('message', '')


@login_manager.user_loader
def load_user(user_id):
    return User(user_id=user_id)


@login_manager.unauthorized_handler
def unauthorized_callback():
    flask.flash("Unauthorized, login required.")
    return flask.redirect(flask.url_for('login'))


@app.route("/", methods=["GET"])
@login_required
def index():
    projects = client.list_projects()
    # contains: project list, actions: create new project, links to project details
    createprojectform = AddProjectForm()
    return render_template(
        'index.html.j2',
        username=current_user.user_id,
        projects=projects['projects'],
        createprojectform=createprojectform,
    )


@app.route("/login/", methods=["GET", "POST"])
def login():
    form = LoginForm()
    if request.method == 'POST' and form.validate():
        auth_result = client.authenticate(form.username.data, form.password.data)

        if auth_result['errno'] == 0:
            login_user(User(user_id=form.username.data))
            next_url = flask.request.args.get('next')
            if not is_safe_url(next_url):
                return flask.abort(400)
            flask.flash(f"Login successful as {form.username.data}.")
            return flask.redirect(next_url or flask.url_for('index'))
        else:
            form.message.text = auth_result['message']
            return render_template('login.html.j2', form=form)
    else:
        form.message.text = ''
        return render_template('login.html.j2', form=form)


@app.route("/project/add/", methods=["GET", "POST"])
@login_required
def add_project():
    form = AddProjectForm()
    if request.method == 'POST' and form.validate():
        result = client.create_project(
            name=form.name.data,
            owner=current_user.user_id,
            members=form.members.data
        )

        if result['errno'] == 0:
            flask.flash(f"Project {form.name.data} added.")
            return flask.redirect(flask.url_for('index'))
        else:
            form.message.text = result['message']
            return render_template(
                'addproject.html.j2',
                createprojectform=form,
                username=current_user.user_id,
            )
    else:
        form.message.text = ''
        return render_template(
            'addproject.html.j2',
            createprojectform=form,
            username=current_user.user_id,
        )


@app.route("/project/details/<name>/", methods=["GET", "POST"])
@login_required
def project(name: str):
    # First perform member action
    addmembersform = AddProjectMembersForm()
    good_form = addmembersform.action.data in ('add', 'remove')
    if request.method == 'POST' and addmembersform.validate() and good_form:
        result = client.change_project_members(
            projectname=name,
            op=addmembersform.action.data,
            members=addmembersform.members.data,
            user=current_user.user_id,
        )
        if result['errno'] == 0:
            if addmembersform.action.data == 'add':
                flask.flash(f"Members {addmembersform.members.data} added.")
            else:
                flask.flash(f"Members {addmembersform.members.data} removed.")
            addmembersform.message.text = ''
        else:
            addmembersform.message.text = result['message']
    else:
        addmembersform.message.text = ''
    # Then load project AFTER action is performed
    pr_info = client.projectdetails(name)
    # Reset for rendering a clean "add" form
    # might be changed through parsing the input of "remove" button form
    addmembersform.action.data = 'add'
    addmembersform.members.data = []
    if pr_info['errno'] == 0:
        return render_template(
            'projectdetails.html.j2',
            username=current_user.user_id,
            project=pr_info['project'],
            datasets=pr_info['datasets'],
            addmembersform=addmembersform,
        )
    else:
        return render_template(
            'error.html.j2',
            username=current_user.user_id,
            result=pr_info
        )


@app.route('/logout')
@login_required
def logout():
    logout_user()
    flask.flash('You were logged out.')
    return flask.redirect(flask.url_for('login'))
